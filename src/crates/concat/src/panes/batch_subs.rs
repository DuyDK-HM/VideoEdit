// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: File > Batch subtitles, and Batch subtitles + translation.
//!
//! For every video clip selected on the timeline - every one, with none
//! selected - its media is subtitled by the old project's PhoWhisper (+
//! Demucs) and, when asked, translated by its Ollama/Gemini pipeline, both
//! through `concat_host::sidecar`. Each clip's subtitles go on as captions
//! where the clip is, trimmed to what it shows, through the Captions
//! sheet's own SRT import (its look, one undo step a clip). A media file
//! used by many clips is subtitled once. File > Stop AI job ends it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use concat_host::sidecar::{self, Bridge};
use concat_project::model::ClipKind;

use crate::host::{on_ui_in_project, project_epoch, spawn_long};
use crate::i18n::tf;
use crate::panes::Msg;
use crate::panes::captions::CaptionsMsg;
use crate::studio::Studio;

/// The running batch's stop switch.
fn running() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static RUNNING: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(None))
}

/// One clip to caption: where it is and what of its media it shows.
struct Target {
    media: PathBuf,
    start: f64,
    source_start: f64,
    /// Media seconds the clip shows.
    duration: f64,
    speed: f64,
}

/// Starts a batch over the selected video clips (or all of them).
pub fn start(studio: &mut Studio, translate: bool) {
    let bridge = match Bridge::from_env() {
        Ok(bridge) => bridge,
        Err(error) => return studio.notify(&error, true),
    };
    let Some(project) = studio
        .session
        .as_ref()
        .map(|session| PathBuf::from(session.path()))
    else {
        return;
    };
    let mut slot = running().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if slot.is_some() {
        return studio.notify(&crate::i18n::t("batchSubs.busy"), true);
    }
    let paths: HashMap<&str, &str> = studio
        .project()
        .media
        .iter()
        .map(|item| (item.id.as_str(), item.path.as_str()))
        .collect();
    let chosen = |id: &String| studio.selection.is_empty() || studio.selection.contains(id);
    let mut targets: Vec<Target> = studio
        .timeline()
        .clips
        .iter()
        .filter(|clip| clip.kind == ClipKind::Video && chosen(&clip.id))
        .filter_map(|clip| {
            Some(Target {
                media: PathBuf::from(paths.get(clip.media_id.as_str())?),
                start: clip.start,
                source_start: clip.source_start,
                // Timeline seconds at the clip's speed are source seconds.
                duration: clip.duration * clip.speed.max(0.01),
                speed: clip.speed.max(0.01),
            })
        })
        .collect();
    targets.sort_by(|a, b| a.start.total_cmp(&b.start));
    if targets.is_empty() {
        return studio.notify(&crate::i18n::t("batchSubs.noClips"), true);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *slot = Some(Arc::clone(&cancel));
    drop(slot);
    studio.notify(&tf("batchSubs.started", &[&targets.len()]), false);
    let epoch = project_epoch();
    let folder = project.join("subtitles");
    // Delivered whatever project is open by then, so the switch is always
    // let go; the captions themselves only land in the project they were for.
    spawn_long(
        move || run(&bridge, &targets, &folder, translate, &cancel, epoch),
        |studio, _, _, (placed, failures): (usize, Vec<String>)| {
            *running().lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            match failures.first() {
                Some(first) => studio.notify(&tf("batchSubs.doneWithErrors", &[&placed, &failures.len(), first]), true),
                None => studio.notify(&tf("batchSubs.done", &[&placed]), false),
            }
        },
    );
}

/// Stops the running batch, if any.
pub fn stop() {
    if let Some(cancel) = running().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
        cancel.store(true, Ordering::Relaxed);
    }
}

/// The worker: one subtitle (and translation) per media, placed on each clip.
fn run(
    bridge: &Bridge,
    targets: &[Target],
    folder: &std::path::Path,
    translate: bool,
    cancel: &AtomicBool,
    epoch: u64,
) -> (usize, Vec<String>) {
    let _ = std::fs::create_dir_all(folder);
    let mut done: HashMap<PathBuf, Result<PathBuf, String>> = HashMap::new();
    let (mut placed, mut failures) = (0, Vec::new());
    let total = targets.len();
    for (index, target) in targets.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            failures.push("cancelled".to_owned());
            break;
        }
        let name = target
            .media
            .file_name()
            .map_or(String::new(), |name| name.to_string_lossy().into_owned());
        let srt = done
            .entry(target.media.clone())
            .or_insert_with(|| subtitle_media(bridge, &target.media, folder, translate, cancel, epoch, &name, index + 1, total))
            .clone();
        let srt = match srt {
            Ok(srt) => srt,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        let out = folder.join(format!("placed-{index}.srt"));
        match sidecar::write_placed(&srt, &out, target.start, target.source_start, target.duration, target.speed) {
            Ok(0) => {}
            Ok(_) => {
                placed += 1;
                on_ui_in_project(epoch, move |studio, _, _| {
                    studio.handle(Msg::Captions(CaptionsMsg::ImportSrtFrom(out)));
                });
            }
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    (placed, failures)
}

#[allow(clippy::too_many_arguments)]
fn subtitle_media(
    bridge: &Bridge,
    media: &std::path::Path,
    folder: &std::path::Path,
    translate: bool,
    cancel: &AtomicBool,
    epoch: u64,
    name: &str,
    number: usize,
    total: usize,
) -> Result<PathBuf, String> {
    let out_dir = folder.join(format!("media-{number}"));
    let source = sidecar::run(
        bridge,
        &["subtitle".as_ref(), "--video".as_ref(), media.as_os_str(), "--out-dir".as_ref(), out_dir.as_os_str()],
        cancel,
        &mut toaster(false, number, total, name, epoch),
    )?;
    if !translate {
        return Ok(source);
    }
    let translated = out_dir.join("translated.srt");
    sidecar::run(
        bridge,
        &["translate".as_ref(), "--srt".as_ref(), source.as_os_str(), "--out".as_ref(), translated.as_os_str()],
        cancel,
        &mut toaster(true, number, total, name, epoch),
    )
}

/// Progress for one step as a toast every few seconds at most: the bridge
/// reports far more often than anyone reads.
fn toaster(
    translating: bool,
    number: usize,
    total: usize,
    name: &str,
    epoch: u64,
) -> impl FnMut(Option<f32>, &str) {
    let name = name.to_owned();
    let mut last: Option<std::time::Instant> = None;
    move |percent, _| {
        if last.is_some_and(|at| at.elapsed().as_secs() < 5) {
            return;
        }
        last = Some(std::time::Instant::now());
        let percent = percent.map_or(String::from("…"), |p| format!("{p:.0}%"));
        let args: [&dyn std::fmt::Display; 4] = [&number, &total, &name, &percent];
        let text = if translating {
            tf("batchSubs.translating", &args)
        } else {
            tf("batchSubs.subtitling", &args)
        };
        on_ui_in_project(epoch, move |studio, _, _| studio.notify(&text, false));
    }
}
