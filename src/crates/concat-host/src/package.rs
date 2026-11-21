// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: a project and every file it uses, packed into one `.zip` that
//! opens whole on another machine.
//!
//! The document names its files by absolute path (media, fonts, LUTs), in
//! places that move between document versions. So rather than know the
//! layout, packing walks every string in `concat.json`: one that is an
//! absolute path to a file goes in the package - under its place in the
//! project folder, or under `external/` when it lives elsewhere - and the
//! string becomes `pkg://<path in the package>`. Unpacking turns each
//! `pkg://` back into an absolute path where it was unpacked.
//!
//! `cache/` (proxies, waveforms) and the lock are left out: they are
//! rebuilt. The zip is written and read by `tar`, which Windows 10 and
//! later ship (bsdtar), so no library is added for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub(crate) const DOCUMENT: &str = "concat.json";
pub(crate) const PREFIX: &str = "pkg://";
/// Left out of a package: rebuilt where it is opened.
const SKIPPED: &[&str] = &["cache", "concat.lock"];

/// Calls `change` on every string in `value`, replacing it when it returns one.
pub(crate) fn rewrite(value: &mut Value, change: &mut dyn FnMut(&str) -> Option<String>) {
    match value {
        Value::String(text) => {
            if let Some(new) = change(text) {
                *text = new;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite(item, change)),
        Value::Object(map) => map.values_mut().for_each(|item| rewrite(item, change)),
        _ => {}
    }
}

fn copy_tree(from: &Path, to: &Path, top: bool) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if top && SKIPPED.iter().any(|skip| name == *skip) {
            continue;
        }
        let (source, target) = (entry.path(), to.join(&name));
        if source.is_dir() {
            copy_tree(&source, &target, false)?;
        } else {
            std::fs::copy(&source, &target)
                .map_err(|error| format!("{}: {error}", source.display()))?;
        }
    }
    Ok(())
}

fn tar(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = Command::new("tar");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|error| format!("the tar program is needed for packages ({error})"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("tar: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// Packs the project in `project` into the zip `out`. Returns how many
/// files from outside the project folder went in with it.
pub fn pack(project: &Path, out: &Path) -> Result<usize, String> {
    let document = project.join(DOCUMENT);
    let text = std::fs::read_to_string(&document)
        .map_err(|error| format!("{}: {error}", document.display()))?;
    let mut value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let staging = out.with_extension("packing");
    let _ = std::fs::remove_dir_all(&staging);
    copy_tree(project, &staging, true)?;

    let mut external: HashMap<PathBuf, String> = HashMap::new();
    let mut failure = None;
    rewrite(&mut value, &mut |text| {
        let path = Path::new(text);
        if !path.is_absolute() || !path.is_file() {
            return None;
        }
        if let Ok(inside) = path.strip_prefix(project) {
            return Some(format!("{PREFIX}{}", inside.to_string_lossy().replace('\\', "/")));
        }
        if let Some(packed) = external.get(path) {
            return Some(packed.clone());
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        let packed = format!("external/{}-{name}", external.len() + 1);
        if let Err(error) = std::fs::create_dir_all(staging.join("external"))
            .and_then(|()| std::fs::copy(path, staging.join(&packed)).map(drop))
        {
            failure.get_or_insert(format!("{}: {error}", path.display()));
            return None;
        }
        let packed = format!("{PREFIX}{packed}");
        external.insert(path.to_path_buf(), packed.clone());
        Some(packed)
    });
    let result = failure.map_or(Ok(()), Err).and_then(|()| {
        let json = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
        std::fs::write(staging.join(DOCUMENT), json).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(out);
        tar(&["-a".as_ref(), "-c".as_ref(), "-f".as_ref(), out.as_os_str(), "-C".as_ref(), staging.as_os_str(), ".".as_ref()])
    });
    let _ = std::fs::remove_dir_all(&staging);
    result.map(|()| external.len())
}

/// Unpacks the zip `package` into the new folder `into` and points the
/// document at the files there. Returns the project folder, `into`.
pub fn unpack(package: &Path, into: &Path) -> Result<PathBuf, String> {
    if into.exists() {
        return Err(format!("{} is already there", into.display()));
    }
    std::fs::create_dir_all(into).map_err(|error| format!("{}: {error}", into.display()))?;
    tar(&["-x".as_ref(), "-f".as_ref(), package.as_os_str(), "-C".as_ref(), into.as_os_str()])?;
    let document = into.join(DOCUMENT);
    let text = std::fs::read_to_string(&document)
        .map_err(|_| format!("{} holds no Concat project", package.display()))?;
    let mut value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    rewrite(&mut value, &mut |text| {
        let packed = text.strip_prefix(PREFIX)?;
        Some(into.join(packed).to_string_lossy().into_owned())
    });
    let json = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(&document, json).map_err(|error| error.to_string())?;
    Ok(into.to_path_buf())
}

/// A folder beside `package` named after it that does not exist yet.
pub fn unpack_folder_for(package: &Path) -> PathBuf {
    let stem = package
        .file_stem()
        .map_or("project".to_owned(), |stem| stem.to_string_lossy().into_owned());
    let parent = package.parent().unwrap_or(Path::new("."));
    let mut folder = parent.join(&stem);
    let mut number = 2;
    while folder.exists() {
        folder = parent.join(format!("{stem} {number}"));
        number += 1;
    }
    folder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_absolute_file_path_in_the_document_is_rewritten() {
        let mut value = serde_json::json!({
            "media": [{"path": "C:/x/a.mp4"}, {"path": "relative.mp4"}],
            "fonts": {"title": "C:/x/f.ttf"}, "count": 3
        });
        let mut seen = Vec::new();
        rewrite(&mut value, &mut |text| {
            seen.push(text.to_owned());
            text.starts_with("C:").then(|| format!("{PREFIX}{text}"))
        });
        assert_eq!(seen.len(), 3);
        assert_eq!(value["media"][0]["path"], "pkg://C:/x/a.mp4");
        assert_eq!(value["media"][1]["path"], "relative.mp4");
        assert_eq!(value["fonts"]["title"], "pkg://C:/x/f.ttf");
    }

    /// Packed on one machine, unpacked somewhere else: the document points
    /// at the unpacked copies, the cache stays behind.
    #[cfg(windows)]
    #[test]
    fn a_package_opens_whole_somewhere_else() {
        let root = std::env::temp_dir().join(format!("concat-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (project, outside) = (root.join("project"), root.join("outside"));
        std::fs::create_dir_all(project.join("audio")).unwrap();
        std::fs::create_dir_all(project.join("cache")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(project.join("audio/voice.wav"), b"wav").unwrap();
        std::fs::write(project.join("cache/proxy.mp4"), b"big").unwrap();
        std::fs::write(outside.join("clip.mp4"), b"mp4").unwrap();
        let document = serde_json::json!({"media": [
            {"path": project.join("audio/voice.wav")},
            {"path": outside.join("clip.mp4")},
            {"path": outside.join("clip.mp4")},
        ]});
        std::fs::write(project.join(DOCUMENT), document.to_string()).unwrap();

        let zip = root.join("project.zip");
        assert_eq!(pack(&project, &zip).unwrap(), 1, "one outside file, packed once");
        let opened = unpack(&zip, &unpack_folder_for(&zip)).unwrap();
        assert_eq!(opened, root.join("project 2"));
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(opened.join(DOCUMENT)).unwrap()).unwrap();
        for item in value["media"].as_array().unwrap() {
            let path = PathBuf::from(item["path"].as_str().unwrap());
            assert!(path.starts_with(&opened) && path.is_file(), "{}", path.display());
        }
        assert!(!opened.join("cache").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
