// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: videos downloaded from a link (TikTok, YouTube, Douyin, a playlist
//! or a whole channel) with the `yt-dlp` program, into the project's
//! `downloads/` folder, for the window to import.
//!
//! `yt-dlp` is a program on PATH (setup.bat installs it), run as a child
//! process. Its progress and finished files are read from its stdout, one
//! line each, in the templates below. A cookie file in Netscape format is
//! passed on without TikTok's WAF cookies (`_waf...`): a browser's export
//! carries a WAF token tied to that browser, and sending it gets a 403 on
//! the first page.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const PROGRESS: &str = "CONCAT-PROGRESS ";
const FILE: &str = "CONCAT-FILE ";

/// What one line of `yt-dlp`'s output says.
#[derive(Debug, PartialEq)]
pub enum Line {
    /// The file being downloaded is this far along, 0 to 100.
    Progress(f32),
    /// A video is finished, merged and moved to this path.
    Finished(PathBuf),
    /// An error for one item; the others go on.
    Error(String),
    /// Anything else.
    Other,
}

/// Reads one line of output.
pub fn parse(line: &str) -> Line {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(PROGRESS) {
        return rest
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse()
            .map_or(Line::Other, Line::Progress);
    }
    if let Some(rest) = line.strip_prefix(FILE) {
        return Line::Finished(PathBuf::from(rest.trim()));
    }
    if let Some(rest) = line.strip_prefix("ERROR:") {
        return Line::Error(rest.trim().to_owned());
    }
    Line::Other
}

/// `cookies` (Netscape format) without its WAF cookies, and the names of
/// the ones taken out. Comments and blank lines are kept as they are.
pub fn without_waf_cookies(cookies: &str) -> (String, Vec<String>) {
    let mut kept = String::with_capacity(cookies.len());
    let mut dropped = Vec::new();
    for line in cookies.lines() {
        let name = line.split('\t').nth(5).unwrap_or_default();
        if !line.starts_with('#') && name.starts_with("_waf") {
            dropped.push(name.to_owned());
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    (kept, dropped)
}

/// What a download reports while it runs.
pub enum Event {
    /// The current file's progress, 0 to 100.
    Progress(f32),
    /// One more video is in.
    Finished(PathBuf),
}

/// The outcome: the files downloaded, and the errors of the items that
/// were not. Cancelled downloads return what had finished by then.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The videos downloaded, in the order they finished.
    pub files: Vec<PathBuf>,
    /// One line per item that failed, as yt-dlp said it.
    pub errors: Vec<String>,
    /// Stopped before the end.
    pub cancelled: bool,
}

/// Downloads every video `urls` name into `folder`, through `cookies` if
/// given, telling `report` as it goes; stops when `cancel` is set. An Err
/// is a download that could not start at all (no `yt-dlp`, no folder).
pub fn download(
    urls: &[String],
    cookies: Option<&Path>,
    folder: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Event),
) -> Result<Outcome, String> {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut command = Command::new("yt-dlp");
    command
        .args(["--newline", "--no-colors", "--ignore-errors", "--progress", "--restrict-filenames"])
        // UTF-8 out, whatever the console's code page: a project folder
        // named in Vietnamese is in every path it prints.
        .args(["--encoding", "utf-8"])
        .env("PYTHONIOENCODING", "utf-8")
        .args(["-f", "bv*+ba/b", "--merge-output-format", "mp4"])
        .arg("-o")
        .arg(folder.join("%(extractor_key)s_%(id)s.%(ext)s"))
        .args(["--progress-template", &format!("download:{PROGRESS}%(progress._percent_str)s")])
        .args(["--print", &format!("after_move:{FILE}%(filepath)s")]);
    // In the system's temp folder, never the project's (a package or a sync
    // would carry the session cookies along), and deleted however this ends.
    let mut filtered = None;
    if let Some(cookies) = cookies {
        let text = std::fs::read_to_string(cookies)
            .map_err(|error| format!("{}: {error}", cookies.display()))?;
        let (kept, dropped) = without_waf_cookies(&text);
        if !dropped.is_empty() {
            log::info!("download: left out WAF cookies {}", dropped.join(", "));
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!("concat-cookies-{}-{stamp}.txt", std::process::id()));
        std::fs::write(&path, kept).map_err(|error| format!("{}: {error}", path.display()))?;
        command.arg("--cookies").arg(&path);
        filtered = Some(Removed(path));
    }
    command.args(urls).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("the yt-dlp program is needed to download ({error})"))?;

    // Both streams read on their own threads into one channel, so a cancel
    // is seen within a tick whatever the program is doing.
    let (lines, incoming) = mpsc::channel::<String>();
    for stream in [
        child.stdout.take().map(|out| Box::new(out) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|err| Box::new(err) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let lines = lines.clone();
        std::thread::spawn(move || {
            // Bytes split at newlines and read leniently: one line that is
            // not UTF-8 must not end the reading of the rest.
            for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line).trim_end_matches('\r').to_owned();
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(lines);

    let mut outcome = Outcome::default();
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            outcome.cancelled = true;
            break;
        }
        match incoming.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => match parse(&line) {
                Line::Progress(percent) => report(Event::Progress(percent)),
                Line::Finished(path) => {
                    report(Event::Finished(path.clone()));
                    outcome.files.push(path);
                }
                Line::Error(error) => outcome.errors.push(error),
                Line::Other => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.wait();
    drop(filtered);
    Ok(outcome)
}

/// A file deleted when this goes out of scope.
struct Removed(std::path::PathBuf);

impl Drop for Removed {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Ends `child` and what it started (the ffmpeg merging a video).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .creation_flags(0x0800_0000)
            .status();
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_read_by_their_templates() {
        assert_eq!(parse("CONCAT-PROGRESS  42.5%"), Line::Progress(42.5));
        assert_eq!(
            parse("CONCAT-FILE C:\\p\\downloads\\TikTok_123.mp4"),
            Line::Finished(PathBuf::from("C:\\p\\downloads\\TikTok_123.mp4"))
        );
        assert_eq!(
            parse("ERROR: [youtube] abc: Video unavailable"),
            Line::Error("[youtube] abc: Video unavailable".to_owned())
        );
        assert_eq!(parse("[download] Destination: x.mp4"), Line::Other);
        assert_eq!(parse("CONCAT-PROGRESS N/A"), Line::Other);
    }

    #[test]
    fn waf_cookies_are_left_out_and_the_rest_kept() {
        let cookies = "# Netscape HTTP Cookie File\n\
            .tiktok.com\tTRUE\t/\tTRUE\t0\t_waftokenid\tabc\n\
            .tiktok.com\tTRUE\t/\tTRUE\t0\tsessionid\txyz\n";
        let (kept, dropped) = without_waf_cookies(cookies);
        assert_eq!(dropped, ["_waftokenid"]);
        assert!(kept.contains("sessionid") && kept.starts_with("# Netscape"));
        assert!(!kept.contains("_waftokenid"));
    }
}
