// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: the old project's AI (PhoWhisper + Demucs subtitles, Ollama/Gemini
//! translation) run as a child process through `sidecar/old_project_bridge.py`.
//!
//! Where things are comes from the environment, which `setup.bat` writes into
//! `vendor\env.cmd` when it finds the old project beside this repository:
//! `CONCAT_OLD_ROOT` (the old project's folder), `CONCAT_SIDECAR_PYTHON` (its
//! `AIVoice\.venv` Python) and `CONCAT_SIDECAR_BRIDGE` (the bridge script).
//! The bridge prints one JSON object a line; see its docstring.
//!
//! One job at a time: the GPU holds one model, so a second request waits on
//! [`GPU`] rather than loading another next to it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use concat_project::srt::{self, Cue};

/// Held by the job using the GPU.
static GPU: Mutex<()> = Mutex::new(());

/// The paths the bridge needs, from the environment.
#[derive(Clone, Debug)]
pub struct Bridge {
    /// The old project's AIVoice Python.
    pub python: PathBuf,
    /// `sidecar/old_project_bridge.py`.
    pub script: PathBuf,
    /// The old project's folder.
    pub old_root: PathBuf,
}

impl Bridge {
    /// The configured bridge, or what is missing, in words.
    pub fn from_env() -> Result<Self, String> {
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.exists())
                .ok_or_else(|| format!("{name} is not set to an existing path (run setup.bat with the old project beside this repository)"))
        };
        Ok(Self {
            python: var("CONCAT_SIDECAR_PYTHON")?,
            script: var("CONCAT_SIDECAR_BRIDGE")?,
            old_root: var("CONCAT_OLD_ROOT")?,
        })
    }
}

/// What one line of the bridge's output says.
#[derive(Debug, PartialEq)]
pub enum Line {
    /// How far along (0 to 100, when it knows) and what it is doing.
    Progress(Option<f32>, String),
    /// Something to log; the job goes on.
    Warning(String),
    /// The `.srt` written.
    Done(PathBuf),
    /// Why it stopped.
    Error(String),
    /// Not one of the bridge's lines.
    Other,
}

/// Reads one line of the bridge's output.
pub fn parse(line: &str) -> Line {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return Line::Other;
    };
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    match value["type"].as_str() {
        Some("progress") => Line::Progress(value["percent"].as_f64().map(|p| p as f32), text("message")),
        Some("warning") => Line::Warning(text("message")),
        Some("done") => Line::Done(PathBuf::from(text("srt"))),
        Some("error") => Line::Error(text("message")),
        _ => Line::Other,
    }
}

/// Runs the bridge with `args`, reporting progress; the `.srt` it wrote, or
/// why not. Waits for the GPU first. A set `cancel` ends the process tree.
pub fn run(
    bridge: &Bridge,
    args: &[&std::ffi::OsStr],
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Option<f32>, &str),
) -> Result<PathBuf, String> {
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_owned());
    }
    let mut command = Command::new(&bridge.python);
    command
        .arg("-X")
        .arg("utf8")
        .arg(&bridge.script)
        .args(args)
        .arg("--old-root")
        .arg(&bridge.old_root)
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| format!("{}: {error}", bridge.python.display()))?;
    let stdout = child.stdout.take().ok_or("no output from the bridge")?;
    let (lines, incoming) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let mut outcome = Err("the bridge ended without a result".to_owned());
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            outcome = Err("cancelled".to_owned());
            break;
        }
        match incoming.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => match parse(&line) {
                Line::Progress(percent, message) => report(percent, &message),
                Line::Warning(message) => log::warn!("sidecar: {message}"),
                Line::Done(path) => outcome = Ok(path),
                Line::Error(message) => outcome = Err(message),
                Line::Other => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.wait();
    outcome
}

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

/// The cues of a media file's subtitles as they fall on a clip of it: the
/// clip shows the media from `source_start` for `duration` seconds (at
/// normal speed) from `start` on the timeline. Cues outside are dropped,
/// cues across an edge are cut at it.
pub fn place_on_clip(cues: &[Cue], start: f64, source_start: f64, duration: f64) -> Vec<Cue> {
    let (from, to) = (source_start, source_start + duration);
    cues.iter()
        .filter(|cue| cue.end > from && cue.start < to)
        .map(|cue| Cue {
            start: cue.start.max(from) - from + start,
            end: cue.end.min(to) - from + start,
            text: cue.text.clone(),
        })
        .collect()
}

/// Writes `srt_file`'s cues placed on a clip (see [`place_on_clip`]) to
/// `out`, for the captions import to read.
pub fn write_placed(
    srt_file: &Path,
    out: &Path,
    start: f64,
    source_start: f64,
    duration: f64,
) -> Result<usize, String> {
    let bytes = std::fs::read(srt_file).map_err(|error| format!("{}: {error}", srt_file.display()))?;
    let cues = srt::parse(&String::from_utf8_lossy(&bytes)).map_err(|error| error.to_string())?;
    let placed = place_on_clip(&cues, start, source_start, duration);
    std::fs::write(out, srt::write(&placed)).map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(placed.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_lines_are_read() {
        assert_eq!(
            parse(r#"{"type": "progress", "percent": 12.5, "message": "Whisper"}"#),
            Line::Progress(Some(12.5), "Whisper".to_owned())
        );
        assert_eq!(parse(r#"{"type": "progress", "percent": null, "message": ""}"#), Line::Progress(None, String::new()));
        assert_eq!(parse(r#"{"type": "done", "srt": "C:/a.srt"}"#), Line::Done(PathBuf::from("C:/a.srt")));
        assert_eq!(parse(r#"{"type": "error", "message": "no GPU"}"#), Line::Error("no GPU".to_owned()));
        assert_eq!(parse("Loading model..."), Line::Other);
    }

    #[test]
    fn cues_follow_the_clip_and_its_trim() {
        let cue = |start: f64, end: f64, text: &str| Cue { start, end, text: text.to_owned() };
        let cues = [cue(0.0, 2.0, "before"), cue(4.0, 6.0, "across"), cue(7.0, 8.0, "inside"), cue(20.0, 21.0, "after")];
        // The clip shows media seconds 5..15 from timeline second 100.
        let placed = place_on_clip(&cues, 100.0, 5.0, 10.0);
        assert_eq!(placed, [cue(100.0, 101.0, "across"), cue(102.0, 103.0, "inside")]);
    }
}
