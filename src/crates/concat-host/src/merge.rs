// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: merging many files into one, in the order of their names.
//!
//! Files that match each other - same video codec, size and frame rate,
//! and the same audio codec, rate and channels, or all without sound - are
//! joined by the `ffmpeg` program with `-c copy`: no re-encode, seconds for
//! an hour of footage. Anything else goes on the timeline one after
//! another, where the export does the scaling, padding and silence.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::media::MediaSummary;

/// Orders names the way a person reads them: the runs of digits by their
/// value, so `2_a.mp4` comes before `10_a.mp4`; letters without case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Whether `files` can be joined without a re-encode: two or more videos
/// whose streams all match the first's.
pub fn can_copy(files: &[MediaSummary]) -> bool {
    let key = |file: &MediaSummary| {
        let video = file.video.as_ref()?;
        let audio = file
            .audio
            .as_ref()
            .map(|audio| (audio.codec.clone(), audio.sample_rate, audio.channels));
        Some((
            video.codec.clone(),
            video.width,
            video.height,
            video.frame_rate_fraction.clone(),
            audio,
        ))
    };
    let Some(first) = files.first().and_then(key) else {
        return false;
    };
    files.len() > 1 && files.iter().all(|file| key(file).as_ref() == Some(&first))
}

/// Joins `files` (in that order) into `target` with `ffmpeg -c copy`.
pub fn concat_copy(files: &[PathBuf], target: &Path) -> Result<(), String> {
    let folder = target.parent().ok_or("no folder to write into")?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    // The concat demuxer's list: one `file '...'` line each, quotes escaped.
    let list = folder.join(".merge-list.txt");
    let lines: String = files
        .iter()
        .map(|file| format!("file '{}'\n", file.to_string_lossy().replace('\'', "'\\''")))
        .collect();
    std::fs::write(&list, lines).map_err(|error| format!("{}: {error}", list.display()))?;
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(target);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output();
    let _ = std::fs::remove_file(&list);
    let output = output.map_err(|error| format!("the ffmpeg program is needed to merge ({error})"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(target);
        let reason = String::from_utf8_lossy(&output.stderr);
        return Err(format!("merge failed: {}", reason.lines().last().unwrap_or("no output").trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AudioStreamInfo, VideoStreamInfo};

    #[test]
    fn names_sort_by_the_value_of_their_numbers() {
        let mut names = vec!["10_end.mp4", "2_b.mp4", "001_start.mp4", "2_A.mp4", "clip.mp4"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["001_start.mp4", "2_A.mp4", "2_b.mp4", "10_end.mp4", "clip.mp4"]);
    }

    fn file(codec: &str, width: u32, audio: Option<&str>) -> MediaSummary {
        MediaSummary {
            path: String::new(),
            duration: Some(1.0),
            kind: concat_project::model::MediaKind::Video,
            video: Some(VideoStreamInfo {
                index: 0,
                codec: codec.to_owned(),
                width,
                height: 1080,
                frame_rate: 30.0,
                frame_rate_fraction: "30/1".to_owned(),
                color_range: None,
                color_space: Default::default(),
            }),
            audio: audio.map(|codec| AudioStreamInfo {
                index: 1,
                codec: codec.to_owned(),
                sample_rate: 48_000,
                channels: 2,
                title: String::new(),
                language: String::new(),
            }),
            audio_tracks: Vec::new(),
        }
    }

    #[test]
    fn only_matching_videos_are_copied() {
        let same = [file("h264", 1920, Some("aac")), file("h264", 1920, Some("aac"))];
        assert!(can_copy(&same));
        assert!(!can_copy(&same[..1]), "one file is no merge");
        assert!(!can_copy(&[file("h264", 1920, Some("aac")), file("h264", 1280, Some("aac"))]));
        assert!(!can_copy(&[file("h264", 1920, Some("aac")), file("hevc", 1920, Some("aac"))]));
        assert!(!can_copy(&[file("h264", 1920, Some("aac")), file("h264", 1920, None)]));
    }
}
