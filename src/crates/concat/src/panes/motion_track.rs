// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: File > Track motion. The selected clip (a title, sticker, shape or
//! picture), placed over something in the video beneath at its first frame,
//! follows it: two points either side of the clip's centre are tracked
//! through that video (`concat_host::track`) and their motion becomes the
//! clip's OffsetX/OffsetY, Scale and Rotation keys, ten a second, as one
//! undo step.
//!
//! The video beneath is taken to fill the frame (its own scale 1, centred),
//! which is how a phone clip sits on its own canvas; a video moved or
//! scaled on the canvas is tracked as though it were not.

use std::sync::atomic::AtomicBool;

use concat_host::track::{self, Sample};
use concat_project::Command;
use concat_project::model::{ClipKind, KeyEase, KeyProperty};

use crate::host::spawn_long_in_project;
use crate::i18n::{t, tf};
use crate::studio::Studio;

/// Samples a second, and so keys a second.
const PER_SECOND: f64 = 10.0;
/// Half the distance between the two points, as a fraction of the width.
const HALF_SPAN: f64 = 0.05;

/// Tracks the selected clip onto the video beneath it.
pub fn track_selected(studio: &mut Studio) {
    let [id] = studio.selection.as_slice() else {
        return studio.notify(&t("motionTrack.selectOne"), true);
    };
    let id = id.clone();
    let (clip, video) = {
        let timeline = studio.timeline();
        let clip = timeline.clips.iter().find(|clip| clip.id == id).cloned();
        // The video under the clip's first frame, on another lane.
        let video = clip.as_ref().and_then(|clip| {
            timeline
                .clips
                .iter()
                .find(|other| {
                    other.kind == ClipKind::Video
                        && other.track_id != clip.track_id
                        && other.start <= clip.start
                        && clip.start < other.start + other.duration
                })
                .cloned()
        });
        (clip, video)
    };
    let Some(clip) = clip else {
        return;
    };
    if matches!(clip.kind, ClipKind::Video | ClipKind::Audio | ClipKind::SoundLayer | ClipKind::Transition) {
        return studio.notify(&t("motionTrack.selectOne"), true);
    }
    let Some(video) = video else {
        return studio.notify(&t("motionTrack.noVideo"), true);
    };
    let Some((path, aspect)) = studio
        .project()
        .media
        .iter()
        .find(|item| item.id == video.media_id)
        .map(|item| {
            let aspect = match (item.width, item.height) {
                (Some(width), Some(height)) if height > 0 => f64::from(width) / f64::from(height),
                _ => 16.0 / 9.0,
            };
            (item.path.clone(), aspect)
        })
    else {
        return;
    };
    // Tracking runs in the media's seconds; the clip's keys in the
    // timeline's. The video's speed is the rate between the two.
    let speed = video.speed.max(0.01);
    let from = video.source_start + (clip.start - video.start) * speed;
    let length = clip.duration.min(video.start + video.duration - clip.start) * speed;
    let centre = (0.5 + clip.offset_x, 0.5 + clip.offset_y);
    let (a, b) = ((centre.0 - HALF_SPAN, centre.1), (centre.0 + HALF_SPAN, centre.1));
    studio.notify(&t("motionTrack.started"), false);
    spawn_long_in_project(
        move || track::track(&path, from, length, a, b, PER_SECOND, &AtomicBool::new(false)),
        move |studio, _, _, result| match result {
            Ok(samples) if samples.len() > 1 => {
                let count = samples.len();
                studio.apply(Command::Batch { commands: keys(&clip, &samples, aspect, speed) });
                studio.notify(&tf("motionTrack.done", &[&count]), false);
            }
            Ok(_) => studio.notify(&t("motionTrack.tooShort"), true),
            Err(error) => studio.notify(&error, true),
        },
    );
}

/// The keys that make `clip` follow `samples`, replacing its own keys for
/// the four properties.
fn keys(clip: &concat_project::model::Clip, samples: &[Sample], aspect: f64, speed: f64) -> Vec<Command> {
    let properties = [KeyProperty::OffsetX, KeyProperty::OffsetY, KeyProperty::Scale, KeyProperty::Rotation];
    let mut commands: Vec<Command> = properties
        .iter()
        .map(|&property| Command::ClearClipKeys { clip_id: clip.id.clone(), property })
        .collect();
    let first = &samples[0];
    let mut last_turn = 0.0_f64;
    for sample in samples {
        let ((dx, dy), scale, turn) = track::motion(first, sample, aspect);
        // Unwrapped: a thing turning past half a turn keeps turning the
        // same way, rather than its keys jumping a whole turn back.
        let mut turn = turn;
        while turn - last_turn > 180.0 {
            turn -= 360.0;
        }
        while turn - last_turn < -180.0 {
            turn += 360.0;
        }
        last_turn = turn;
        let at = (sample.at / speed / clip.duration.max(1e-6)).clamp(0.0, 1.0);
        let values = [clip.offset_x + dx, clip.offset_y + dy, clip.scale * scale, clip.rotation + turn];
        for (&property, value) in properties.iter().zip(values) {
            commands.push(Command::SetClipKey {
                clip_id: clip.id.clone(),
                property,
                at,
                value,
                ease: KeyEase::default(),
            });
        }
    }
    commands
}
