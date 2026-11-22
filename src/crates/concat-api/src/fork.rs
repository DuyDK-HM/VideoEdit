// SPDX-License-Identifier: AGPL-3.0-or-later

//! The fork's methods: downloading from links, merging, subtitles through
//! the old project's sidecar, packages and the sync server - each what the
//! window's File menu does, for a caller on the API. They wait until they
//! are done: none is a job, so a caller simply gets the outcome.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use concat_host::session::EditorView;
use concat_host::sidecar::{self, Bridge};
use concat_host::{download, media, merge, normalize, package, sync};
use concat_project::{Command, srt};
use serde::Serialize;

use crate::{Api, ApiError, ErrorCode, Holder, refused};

/// [`crate::Request::MediaDownload`]'s outcome.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fetched {
    /// The files downloaded and imported, in the order they finished.
    pub files: Vec<String>,
    /// One line per link or item that failed, as yt-dlp said it.
    pub errors: Vec<String>,
}

/// One subtitle on the timeline.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct Caption {
    /// When it appears, in timeline seconds.
    pub start: f64,
    /// When it goes, in timeline seconds.
    pub end: f64,
    /// The words; a newline breaks the line.
    pub text: String,
}

/// [`crate::Request::CaptionsTranscribe`]'s outcome.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cues {
    /// The media's whole subtitles, as the sidecar wrote them.
    pub srt: String,
    /// The cues the clip shows, placed on the timeline.
    pub captions: Vec<Caption>,
}

/// [`crate::Request::ProjectPack`]'s outcome.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Packed {
    /// The `.zip` written.
    pub package: String,
    /// How many files from outside the project folder it carries.
    pub outside: usize,
}

/// [`crate::Request::SyncPush`]'s outcome.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pushed {
    /// The project's id on the server, for `sync.pull`.
    pub id: String,
    /// The bytes uploaded.
    pub uploaded: u64,
    /// The files uploaded; the rest the server had.
    pub files: usize,
}

/// Nothing stops these methods once begun: a caller's wait is the limit.
static NEVER: AtomicBool = AtomicBool::new(false);

impl Api {
    /// [`crate::Request::MediaImport`]'s file, made readable first: an
    /// iPhone HEIC becomes a JPG in the project's `converted` folder.
    pub(crate) fn importable(&self, path: &str, file: &str) -> Result<String, ApiError> {
        let source = Path::new(file);
        if !normalize::needed(source) {
            return Ok(file.to_owned());
        }
        let project = PathBuf::from(self.session(path)?.path());
        normalize::prepare(&project, source)
            .map(|converted| converted.to_string_lossy().into_owned())
            .map_err(ApiError::failed)
    }

    /// [`crate::Request::MediaDownload`].
    pub fn download(&mut self, path: &str, urls: &[String], cookies: Option<&str>) -> Result<Fetched, ApiError> {
        if urls.is_empty() {
            return Err(ApiError::invalid("name at least one link"));
        }
        let folder = PathBuf::from(self.session(path)?.path()).join("downloads");
        let outcome = download::download(urls, cookies.map(Path::new), &folder, &NEVER, &mut |_| {})
            .map_err(ApiError::failed)?;
        let mut fetched = Fetched { files: Vec::new(), errors: outcome.errors };
        for file in outcome.files {
            let file = file.to_string_lossy().into_owned();
            match self.import(path, &file) {
                Ok(_) => fetched.files.push(file),
                Err(error) => fetched.errors.push(format!("{file}: {}", error.message)),
            }
        }
        Ok(fetched)
    }

    /// [`crate::Request::MediaMerge`].
    pub fn merge(&mut self, path: &str, media_ids: &[String]) -> Result<EditorView, ApiError> {
        if media_ids.len() < 2 {
            return Err(ApiError::invalid("name at least two media items"));
        }
        let session = self.session(path)?;
        let mut picked = media_ids
            .iter()
            .map(|id| {
                session
                    .project()
                    .media
                    .iter()
                    .find(|item| &item.id == id)
                    .map(|item| (item.id.clone(), PathBuf::from(&item.path)))
                    .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no media item {id}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        picked.sort_by(|(_, a), (_, b)| merge::natural_cmp(&file_name(a), &file_name(b)));
        let summaries = picked
            .iter()
            .map(|(_, file)| media::probe(&file.to_string_lossy()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::failed)?;
        if !merge::can_copy(&summaries) {
            let start = session
                .project()
                .active()
                .clips
                .iter()
                .fold(0.0_f64, |end, clip| end.max(clip.start + clip.duration));
            return self.apply(
                path,
                Command::AddClipsInSequence {
                    media_ids: picked.into_iter().map(|(id, _)| id).collect(),
                    track_id: None,
                    start,
                    ripple: false,
                },
            );
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let extension = picked[0]
            .1
            .extension()
            .map_or("mp4".to_owned(), |e| e.to_string_lossy().to_ascii_lowercase());
        let target = Path::new(session.path())
            .join("merged")
            .join(format!("merged-{stamp}.{extension}"));
        let files: Vec<PathBuf> = picked.into_iter().map(|(_, file)| file).collect();
        merge::concat_copy(&files, &target).map_err(ApiError::failed)?;
        self.import(path, &target.to_string_lossy())
    }

    /// [`crate::Request::CaptionsTranscribe`].
    pub fn transcribe(&mut self, path: &str, clip_id: &str, translate: bool) -> Result<Cues, ApiError> {
        let session = self.session(path)?;
        let project = session.project();
        let clip = project
            .active()
            .clip(clip_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no clip {clip_id} on the timeline")))?;
        let item = project
            .media
            .iter()
            .find(|item| item.id == clip.media_id)
            .ok_or_else(|| ApiError::invalid(format!("clip {clip_id} shows no media file")))?;
        let speed = clip.speed.max(0.01);
        let (start, source_start, duration) = (clip.start, clip.source_start, clip.duration * speed);
        let media = PathBuf::from(&item.path);
        let bridge = Bridge::from_env().map_err(ApiError::failed)?;
        let out_dir = Path::new(session.path()).join("subtitles").join(&item.id);
        let mut quiet = |_: Option<f32>, _: &str| {};
        let mut srt_file = sidecar::run(
            &bridge,
            &["subtitle".as_ref(), "--video".as_ref(), media.as_os_str(), "--out-dir".as_ref(), out_dir.as_os_str()],
            &NEVER,
            &mut quiet,
        )
        .map_err(ApiError::failed)?;
        if translate {
            let translated = out_dir.join("translated.srt");
            srt_file = sidecar::run(
                &bridge,
                &["translate".as_ref(), "--srt".as_ref(), srt_file.as_os_str(), "--out".as_ref(), translated.as_os_str()],
                &NEVER,
                &mut quiet,
            )
            .map_err(ApiError::failed)?;
        }
        let text = std::fs::read(&srt_file)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|error| ApiError::failed(format!("{}: {error}", srt_file.display())))?;
        let cues = srt::parse(&text).map_err(|error| ApiError::failed(error.to_string()))?;
        let captions = sidecar::place_on_clip(&cues, start, source_start, duration, speed)
            .into_iter()
            .map(|cue| Caption { start: cue.start, end: cue.end, text: cue.text })
            .collect();
        Ok(Cues { srt: srt_file.to_string_lossy().into_owned(), captions })
    }

    /// [`crate::Request::ProjectPack`].
    pub fn pack(&mut self, path: &str) -> Result<Packed, ApiError> {
        self.save(path, None)?;
        let project = PathBuf::from(self.session(path)?.path());
        let name = file_name(&project);
        let out = project.with_file_name(format!("{name}.zip"));
        let outside = package::pack(&project, &out).map_err(ApiError::failed)?;
        Ok(Packed { package: out.to_string_lossy().into_owned(), outside })
    }

    /// [`crate::Request::ProjectUnpack`].
    pub fn unpack(&mut self, file: &str) -> Result<EditorView, ApiError> {
        let zip = Path::new(file);
        let folder = package::unpack_folder_for(zip);
        self.writable(&folder.to_string_lossy())?;
        let folder = package::unpack(zip, &folder).map_err(ApiError::failed)?;
        self.open(&folder.to_string_lossy())
    }

    /// [`crate::Request::SyncPush`].
    pub fn push(&mut self, path: &str) -> Result<Pushed, ApiError> {
        let server = sync::Server::from_env().map_err(ApiError::failed)?;
        self.save(path, None)?;
        let project = PathBuf::from(self.session(path)?.path());
        let (id, uploaded, files) = sync::push(&server, &project, &mut |_| {}).map_err(ApiError::failed)?;
        Ok(Pushed { id, uploaded, files })
    }

    /// [`crate::Request::SyncPull`].
    pub fn pull(&mut self, id: Option<&str>) -> Result<EditorView, ApiError> {
        let server = sync::Server::from_env().map_err(ApiError::failed)?;
        let id = match id {
            Some(id) => id.to_owned(),
            None => sync::latest(&server)
                .map_err(ApiError::failed)?
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "nothing has been pushed to the sync server"))?,
        };
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .ok_or_else(|| ApiError::failed("no home folder"))?;
        let folder = home.join("Concat Sync").join(&id);
        let key = folder.to_string_lossy().into_owned();
        self.writable(&key)?;
        // Held while the files land, so neither side opens it half written;
        // the window or another process holding it refuses the pull.
        if self.sessions.contains_key(&crate::key_of(&key)) {
            return Err(ApiError::new(ErrorCode::Refused, format!("{key} is open; close it before pulling")));
        }
        self.open.claim(&key, Holder::Api).map_err(|refusal| refused(format!("{key} is {refusal}")))?;
        let pulled = sync::pull(&server, &id, &folder, &mut |_| {});
        let pulled = match pulled {
            Ok(pulled) => pulled,
            Err(error) => {
                self.open.release(&key);
                return Err(ApiError::failed(error));
            }
        };
        let view = self.open(&pulled.to_string_lossy());
        if view.is_err() {
            self.open.release(&key);
        }
        view
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or("project".to_owned(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{AppDirs, Event, Reply, Request, Response};

    use super::*;

    fn scratch_api() -> (Api, tempfile::TempDir, String) {
        let scratch = tempfile::tempdir().expect("scratch");
        let dirs = AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let mut api = Api::with_dirs(dirs, Arc::new(|_event: Event| {}));
        let created = api.dispatch(Request::ProjectCreate {
            location: scratch.path().to_string_lossy().into_owned(),
            name: "Cut".to_owned(),
            video: None,
        });
        assert!(matches!(created, Response::Result(_)), "created");
        let path = scratch.path().join("Cut").to_string_lossy().into_owned();
        (api, scratch, path)
    }

    fn code(response: Response) -> ErrorCode {
        match response {
            Response::Result(_) => panic!("worked, unexpectedly"),
            Response::Error(error) => error.code,
        }
    }

    #[test]
    fn wrong_arguments_are_refused_before_anything_runs() {
        let (mut api, _scratch, path) = scratch_api();
        let one = vec!["m1".to_owned()];
        assert_eq!(code(api.dispatch(Request::MediaMerge { path: path.clone(), media_ids: one })), ErrorCode::Invalid);
        let unknown = vec!["m1".to_owned(), "m2".to_owned()];
        assert_eq!(code(api.dispatch(Request::MediaMerge { path: path.clone(), media_ids: unknown })), ErrorCode::NotFound);
        let none = Request::MediaDownload { path: path.clone(), urls: Vec::new(), cookies: None };
        assert_eq!(code(api.dispatch(none)), ErrorCode::Invalid);
        let clip = Request::CaptionsTranscribe { path, clip_id: "nope".to_owned(), translate: false };
        assert_eq!(code(api.dispatch(clip)), ErrorCode::NotFound);
    }

    #[test]
    fn a_packed_project_unpacks_and_opens() {
        let (mut api, _scratch, path) = scratch_api();
        let Response::Result(Reply::Packed(packed)) = api.dispatch(Request::ProjectPack { path }) else {
            panic!("packed");
        };
        assert!(Path::new(&packed.package).is_file());
        let opened = api.dispatch(Request::ProjectUnpack { file: packed.package });
        assert!(matches!(opened, Response::Result(Reply::View(_))), "opened");
    }
}
