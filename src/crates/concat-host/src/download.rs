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
        .args(["-f", "bv*+ba/b", "--merge-output-format", "mp4"])
        .arg("-o")
        .arg(folder.join("%(extractor_key)s_%(id)s.%(ext)s"))
        .args(["--progress-template", &format!("download:{PROGRESS}%(progress._percent_str)s")])
        .args(["--print", &format!("after_move:{FILE}%(filepath)s")]);
    // Kept for the run, deleted when it ends.
    let mut filtered = None;
    if let Some(cookies) = cookies {
        let text = std::fs::read_to_string(cookies)
            .map_err(|error| format!("{}: {error}", cookies.display()))?;
        let (kept, dropped) = without_waf_cookies(&text);
        if !dropped.is_empty() {
            log::info!("download: left out WAF cookies {}", dropped.join(", "));
        }
        let path = folder.join(".cookies-for-yt-dlp.txt");
        std::fs::write(&path, kept).map_err(|error| format!("{}: {error}", path.display()))?;
        command.arg("--cookies").arg(&path);
        filtered = Some(path);
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
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
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
    if let Some(path) = filtered {
        let _ = std::fs::remove_file(path);
    }
    Ok(outcome)
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
