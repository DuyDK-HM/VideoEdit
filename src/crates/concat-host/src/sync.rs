// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: projects pushed to and pulled from our own server
//! (`server/sync_server.py` on rr), media and all.
//!
//! The document is rewritten as for a package (`crate::package`): every
//! absolute path to a file becomes `pkg://<path>`, under its place in the
//! project folder or under `external/`. Files go up as blobs named by their
//! SHA-256, and only the ones the server lacks, so a clip used by ten
//! projects is uploaded once. A pull fetches only the blobs this machine
//! does not already have in place.
//!
//! The server is `CONCAT_SYNC_URL` (e.g. `http://100.114.205.62:8700`) with
//! `CONCAT_SYNC_TOKEN`, from `vendor\env.cmd`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::package::{DOCUMENT, PREFIX, rewrite};

/// Where the server is and the token it wants.
#[derive(Clone, Debug)]
pub struct Server {
    /// Its base URL, no trailing slash.
    pub url: String,
    /// The bearer token.
    pub token: String,
}

impl Server {
    /// The server named by the environment, or what is missing.
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("CONCAT_SYNC_URL").unwrap_or_default();
        let token = std::env::var("CONCAT_SYNC_TOKEN").unwrap_or_default();
        if url.trim().is_empty() || token.trim().is_empty() {
            return Err("CONCAT_SYNC_URL and CONCAT_SYNC_TOKEN are not set (vendor\\env.cmd)".to_owned());
        }
        Ok(Self { url: url.trim().trim_end_matches('/').to_owned(), token: token.trim().to_owned() })
    }

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(600))
            .build()
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        Self::agent()
            .request(method, &format!("{}{path}", self.url))
            .set("Authorization", &format!("Bearer {}", self.token))
    }
}

/// Sends `body` as JSON and reads the JSON answer.
fn exchange(request: ureq::Request, body: &Value) -> Result<Value, String> {
    let response = request
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(failed)?;
    serde_json::from_reader(response.into_reader()).map_err(|error| error.to_string())
}

/// Reads the JSON answer of a call without a body.
fn fetch(request: ureq::Request) -> Result<Value, String> {
    serde_json::from_reader(request.call().map_err(failed)?.into_reader()).map_err(|error| error.to_string())
}

fn failed(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            format!("server answered {code}: {}", response.into_string().unwrap_or_default())
        }
        other => other.to_string(),
    }
}

/// The SHA-256 of a file, lowercase hex.
pub fn digest(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(|error| format!("{}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A project id the server accepts, from a folder name.
pub fn project_id(folder: &Path) -> String {
    let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let id: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let id = id.trim_matches('-').chars().take(64).collect::<String>();
    if id.is_empty() { "project".to_owned() } else { id }
}

/// The document with its files as `pkg://` paths, and those files by
/// their place in the package.
fn packed(project: &Path) -> Result<(Value, BTreeMap<String, PathBuf>), String> {
    let document = project.join(DOCUMENT);
    let text = std::fs::read_to_string(&document).map_err(|error| format!("{}: {error}", document.display()))?;
    let mut value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let mut files = BTreeMap::new();
    rewrite(&mut value, &mut |text| {
        let path = Path::new(text);
        if !path.is_absolute() || !path.is_file() {
            return None;
        }
        let place = match path.strip_prefix(project) {
            Ok(inside) => inside.to_string_lossy().replace('\\', "/"),
            Err(_) => {
                // Outside the folder: named by where it was, so two files
                // of one name from two folders stay two.
                let mut hasher = Sha256::new();
                hasher.update(text.as_bytes());
                let tag: String = hasher.finalize().iter().take(4).map(|b| format!("{b:02x}")).collect();
                format!("external/{tag}-{}", path.file_name()?.to_string_lossy())
            }
        };
        files.insert(place.clone(), path.to_path_buf());
        Some(format!("{PREFIX}{place}"))
    });
    Ok((value, files))
}

/// Pushes the project in `project` as a new version. Returns the project's
/// id on the server, the version number and how many files were uploaded.
pub fn push(server: &Server, project: &Path, report: &mut dyn FnMut(&str)) -> Result<(String, u64, usize), String> {
    let (document, files) = packed(project)?;
    let mut hashes = BTreeMap::new();
    for (place, path) in &files {
        report(place);
        hashes.insert(place.clone(), digest(path)?);
    }
    let unique: Vec<&String> = hashes.values().collect();
    let answer = exchange(server.request("POST", "/blobs/missing"), &json!({ "hashes": unique }))?;
    let missing: Vec<String> = serde_json::from_value(answer["missing"].clone()).unwrap_or_default();
    for hash in &missing {
        let (place, path) = hashes
            .iter()
            .find(|(_, h)| *h == hash)
            .and_then(|(place, _)| Some((place, files.get(place)?)))
            .ok_or("a missing blob names no file")?;
        report(place);
        let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        server.request("PUT", &format!("/blobs/{hash}")).send(file).map_err(failed)?;
    }
    let id = project_id(project);
    let name = project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| id.clone());
    let answer = exchange(
        server.request("PUT", &format!("/projects/{id}")),
        &json!({ "name": name, "document": document, "files": hashes }),
    )?;
    Ok((id, answer["version"].as_u64().unwrap_or(0), missing.len()))
}

/// The id of the project pushed last, if any.
pub fn latest(server: &Server) -> Result<Option<String>, String> {
    let answer = fetch(server.request("GET", "/projects"))?;
    Ok(answer["projects"][0]["id"].as_str().map(str::to_owned))
}

/// `place` (a path inside a package, `/`-separated) under `into`, or an
/// error for one that would land anywhere else: `..`, a drive, a root, or
/// a backslash, which Windows reads as a separator the server does not.
fn inside(into: &Path, place: &str) -> Result<PathBuf, String> {
    let refused = place.is_empty()
        || place.contains('\\')
        || place.contains(':')
        || place.starts_with('/')
        || place.split('/').any(|part| part == ".." || part == ".");
    if refused {
        return Err(format!("{place}: not a path inside the project"));
    }
    Ok(into.join(place))
}

/// Pulls project `id`'s latest version into `into` (made if need be; files
/// already there with the right digest are kept). Returns `into`.
pub fn pull(server: &Server, id: &str, into: &Path, report: &mut dyn FnMut(&str)) -> Result<PathBuf, String> {
    let mut answer = fetch(server.request("GET", &format!("/projects/{id}")))?;
    let files: BTreeMap<String, String> =
        serde_json::from_value(answer["files"].take()).map_err(|error| error.to_string())?;
    for (place, hash) in &files {
        let target = inside(into, place)?;
        if target.is_file() && digest(&target).is_ok_and(|have| &have == hash) {
            continue;
        }
        report(place);
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        }
        let response = server.request("GET", &format!("/blobs/{hash}")).call().map_err(failed)?;
        let partial = target.with_extension("part");
        let mut out = std::fs::File::create(&partial).map_err(|error| format!("{}: {error}", partial.display()))?;
        std::io::copy(&mut response.into_reader(), &mut out).map_err(|error| error.to_string())?;
        drop(out);
        std::fs::rename(&partial, &target).map_err(|error| format!("{}: {error}", target.display()))?;
    }
    // What was here before is kept beside it: a pull over a copy edited
    // since and not pushed would otherwise lose those edits.
    let document_path = into.join(DOCUMENT);
    if document_path.is_file() {
        let _ = std::fs::copy(&document_path, into.join(format!("{DOCUMENT}.bak")));
    }
    let mut document = answer["document"].take();
    rewrite(&mut document, &mut |text| {
        let place = text.strip_prefix(PREFIX)?;
        Some(into.join(place).to_string_lossy().into_owned())
    });
    std::fs::create_dir_all(into).map_err(|error| format!("{}: {error}", into.display()))?;
    let json = serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?;
    std::fs::write(into.join(DOCUMENT), json).map_err(|error| error.to_string())?;
    Ok(into.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_never_writes_outside_its_folder() {
        let into = Path::new("C:/sync/p");
        assert_eq!(inside(into, "media/a.mp4").unwrap(), into.join("media/a.mp4"));
        for bad in ["../x", "a/../../x", "a\\..\\x", "\\x", "C:/x", "/etc/x", "", "./x"] {
            assert!(inside(into, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ids_are_what_the_server_accepts() {
        assert_eq!(project_id(Path::new("C:/p/Tập 1 (final)")), "T-p-1--final");
        assert_eq!(project_id(Path::new("C:/p/ok_name-2")), "ok_name-2");
        assert_eq!(project_id(Path::new("C:/p/∆∆")), "project");
    }

    #[test]
    fn a_digest_is_the_files_sha256() {
        let path = std::env::temp_dir().join(format!("concat-sync-digest-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            digest(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn files_inside_and_outside_the_project_are_packed() {
        let root = std::env::temp_dir().join(format!("concat-sync-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (project, outside) = (root.join("p"), root.join("o"));
        std::fs::create_dir_all(project.join("audio")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(project.join("audio/v.wav"), b"v").unwrap();
        std::fs::write(outside.join("c.mp4"), b"c").unwrap();
        let document = json!({"media": [{"path": project.join("audio/v.wav")}, {"path": outside.join("c.mp4")}]});
        std::fs::write(project.join(DOCUMENT), document.to_string()).unwrap();
        let (value, files) = packed(&project).unwrap();
        assert_eq!(value["media"][0]["path"], "pkg://audio/v.wav");
        let external = value["media"][1]["path"].as_str().unwrap();
        assert!(external.starts_with("pkg://external/") && external.ends_with("-c.mp4"), "{external}");
        assert_eq!(files.len(), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
