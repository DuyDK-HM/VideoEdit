// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: phone files that FFmpeg's library reads wrong, converted before
//! they are imported.
//!
//! Only HEIC/HEIF needs it. An iPhone photo is a grid of 512-pixel HEVC
//! tiles, and the library's demuxer hands back one tile; the `ffmpeg`
//! program (7.1 and later) composes the grid, so the photo is converted to
//! a JPEG with it. CAF, AMR, MTS and MXF read as they are with the FFmpeg
//! the app links (checked against 8.1), so they are left alone.
//!
//! The JPEG goes into the project's `converted/` folder, not `cache/`,
//! because it is the media the project refers to from then on.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Extensions converted to a JPEG on import.
const CONVERTED_TO_JPEG: &[&str] = &["heic", "heif", "hif"];

/// Whether `path` is converted before import.
pub fn needed(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| CONVERTED_TO_JPEG.contains(&extension.to_ascii_lowercase().as_str()))
}

/// Where the converted copy of `source` goes under `project`: its own name
/// with `.jpg`, numbered when another photo of that name is already there.
pub fn target_for(project: &Path, source: &Path) -> PathBuf {
    let folder = project.join("converted");
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo".to_owned());
    let mut target = folder.join(format!("{stem}.jpg"));
    let mut number = 2;
    while target.exists() {
        target = folder.join(format!("{stem}-{number}.jpg"));
        number += 1;
    }
    target
}

/// The file to import for `source`: `source` itself, or for a HEIC/HEIF a
/// JPEG of it written under `project` by the `ffmpeg` program on PATH.
pub fn prepare(project: &Path, source: &Path) -> Result<PathBuf, String> {
    if !needed(source) {
        return Ok(source.to_path_buf());
    }
    let target = target_for(project, source);
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-n", "-i"])
        .arg(source)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(&target);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up for each photo.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|error| {
        format!("{}: the ffmpeg program is needed to convert it ({error})", source.display())
    })?;
    if !output.status.success() || !target.is_file() {
        let _ = std::fs::remove_file(&target);
        let reason = String::from_utf8_lossy(&output.stderr);
        let reason = reason.lines().last().unwrap_or("no output").trim();
        return Err(format!("{}: could not convert to JPEG ({reason})", source.display()));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_heif_photos_are_converted() {
        assert!(needed(Path::new("IMG_0001.HEIC")));
        assert!(needed(Path::new("a/b.heif")));
        for kept in ["a.caf", "a.amr", "a.MTS", "a.mxf", "a.jpg", "a.mp4", "noextension"] {
            assert!(!needed(Path::new(kept)), "{kept}");
        }
    }

    #[test]
    fn a_converted_photo_never_overwrites_another() {
        let project = std::env::temp_dir().join(format!("concat-normalize-{}", std::process::id()));
        let folder = project.join("converted");
        std::fs::create_dir_all(&folder).unwrap();
        let source = Path::new("phone/IMG_1.HEIC");
        assert_eq!(target_for(&project, source), folder.join("IMG_1.jpg"));
        std::fs::write(folder.join("IMG_1.jpg"), b"x").unwrap();
        assert_eq!(target_for(&project, source), folder.join("IMG_1-2.jpg"));
        std::fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn other_files_are_imported_as_they_are() {
        let source = Path::new("clip.mts");
        assert_eq!(prepare(Path::new("unused"), source).unwrap(), source);
    }
}
