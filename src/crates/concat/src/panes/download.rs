// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: the Download dialog. Links in, videos out into the project's
//! `downloads/` folder through `yt-dlp` (see `concat_host::download`), each
//! imported into the bin as it finishes.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use concat_host::download::{self, Event, Outcome};

use crate::host::{on_ui_in_project, project_epoch, spawn_long_in_project};
use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::panes::media_bin::MediaMsg;
use crate::studio::Studio;
use crate::ui::DownloadData;

#[derive(Debug)]
pub enum DownloadMsg {
    Open,
    Close,
    UrlsEdited(String),
    CookiesPicked(PathBuf),
    CookiesCleared,
    Start,
    Cancel,
    Progress(f32),
    /// One video finished, to be imported now.
    Downloaded(PathBuf),
    Finished(Result<Outcome, String>),
}

#[derive(Default)]
pub struct DownloadPane {
    pub open: bool,
    urls: String,
    cookies: Option<PathBuf>,
    running: bool,
    done: usize,
    percent: f32,
    message: String,
    cancel: Arc<AtomicBool>,
}

/// The links in what was typed or pasted: one per line or space-separated.
pub fn links(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|word| word.starts_with("http://") || word.starts_with("https://"))
        .map(str::to_owned)
        .collect()
}

impl DownloadPane {
    pub fn update(&mut self, msg: DownloadMsg, studio: &mut Studio) {
        match msg {
            DownloadMsg::Open => {
                self.open = true;
                if !self.running {
                    self.message.clear();
                }
            }
            DownloadMsg::Close => {
                // A download keeps going with the dialog closed; its files
                // still land in the bin.
                self.open = false;
            }
            DownloadMsg::UrlsEdited(text) => self.urls = text,
            DownloadMsg::CookiesPicked(path) => self.cookies = Some(path),
            DownloadMsg::CookiesCleared => self.cookies = None,
            DownloadMsg::Start => self.start(studio),
            DownloadMsg::Cancel => self.cancel.store(true, Ordering::Relaxed),
            DownloadMsg::Progress(percent) => self.percent = percent,
            DownloadMsg::Downloaded(path) => {
                self.done += 1;
                self.percent = 0.0;
                studio.handle(Msg::Media(MediaMsg::Import(vec![path])));
            }
            DownloadMsg::Finished(result) => {
                self.running = false;
                self.message = match result {
                    Err(error) => error,
                    Ok(outcome) if outcome.cancelled => {
                        tf("download.cancelled", &[&outcome.files.len()])
                    }
                    Ok(outcome) if outcome.errors.is_empty() => {
                        tf("download.done", &[&outcome.files.len()])
                    }
                    Ok(outcome) => {
                        let first = outcome.errors.first().cloned().unwrap_or_default();
                        tf(
                            "download.doneWithErrors",
                            &[&outcome.files.len(), &outcome.errors.len(), &first],
                        )
                    }
                };
            }
        }
    }

    fn start(&mut self, studio: &mut Studio) {
        let urls = links(&self.urls);
        if urls.is_empty() || self.running {
            self.message = t("download.noLinks");
            return;
        }
        let Some(folder) = studio
            .session
            .as_ref()
            .map(|session| PathBuf::from(session.path()).join("downloads"))
        else {
            return;
        };
        self.running = true;
        self.done = 0;
        self.percent = 0.0;
        self.message.clear();
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let cookies = self.cookies.clone();
        let epoch = project_epoch();
        spawn_long_in_project(
            move || {
                download::download(&urls, cookies.as_deref(), &folder, &cancel, &mut |event| {
                    let msg = match event {
                        Event::Progress(percent) => DownloadMsg::Progress(percent),
                        Event::Finished(path) => DownloadMsg::Downloaded(path),
                    };
                    on_ui_in_project(epoch, move |studio, _, _| {
                        studio.handle(Msg::Download(msg));
                    });
                })
            },
            |studio, _, _, result| studio.handle(Msg::Download(DownloadMsg::Finished(result))),
        );
    }

    pub fn data(&self) -> DownloadData {
        DownloadData {
            open: self.open,
            urls: self.urls.clone().into(),
            cookies: self
                .cookies
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            running: self.running,
            status: if self.running {
                tf("download.running", &[&self.done, &format!("{:.0}", self.percent)]).into()
            } else {
                self.message.clone().into()
            },
            can_start: !self.running && !links(&self.urls).is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_one_per_line_or_among_words() {
        let text = "https://www.tiktok.com/@a/video/1\n\n  https://youtu.be/x  not-a-link\nhttp://b.c";
        assert_eq!(
            links(text),
            ["https://www.tiktok.com/@a/video/1", "https://youtu.be/x", "http://b.c"]
        );
        assert!(links("  \n").is_empty());
    }
}
