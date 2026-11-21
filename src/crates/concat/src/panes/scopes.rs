// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: colour scopes under the colour wheels (ui/fork/scopes.slint).
//!
//! After the monitor shows a paused frame, the same instant is drawn again
//! small on the headless compositor (`Monitor::frame_pixels` - the window's
//! own frame is a GPU texture with no way back), measured by
//! `concat_host::scopes`, and the three images set on the `ForkScopes`
//! global. One at a time: a frame shown while one is being measured asks
//! for one more pass after it, so a scrub ends on the frame it stopped at.
//! Nothing runs while playing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use concat_host::preview::FrameSpec;
use concat_host::scopes::{self, Scope};
use slint::ComponentHandle;

use crate::host::spawn_frame;
use crate::studio::Studio;
use crate::ui::ForkScopes;

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

/// When the pass being measured began. A pass whose answer never came -
/// its project closed under it - stops holding the next one after a while.
static MEASURING: Mutex<Option<Instant>> = Mutex::new(None);
const GIVE_UP: Duration = Duration::from_secs(10);
static AGAIN: AtomicBool = AtomicBool::new(false);

fn image(scope: &Scope) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &scope.pixels,
        scope.width,
        scope.height,
    );
    slint::Image::from_rgba8(buffer)
}

/// Called after the monitor shows a frame at `time`.
pub fn after_frame(studio: &mut Studio, time: f64) {
    if studio.playing {
        return;
    }
    {
        let mut measuring = MEASURING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if measuring.is_some_and(|began| began.elapsed() < GIVE_UP) {
            AGAIN.store(true, Ordering::Release);
            return;
        }
        *measuring = Some(Instant::now());
    }
    let Some((clips, settings)) = studio.preview_clips() else {
        *MEASURING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return;
    };
    let video = &studio.project().active().video;
    let spec = FrameSpec {
        time,
        width: WIDTH,
        height: HEIGHT,
        moving: false,
        proxy: false,
        color_space: video.color_space,
        canvas: video.canvas,
        nearest: false,
    };
    let monitor = studio.host.monitor.clone();
    spawn_frame(
        move || {
            let (pixels, _) = monitor.frame_pixels(clips, &settings, spec)?;
            Ok::<_, String>([
                scopes::waveform(&pixels, WIDTH, HEIGHT, (256, 96)),
                scopes::vectorscope(&pixels, 96),
                scopes::histogram(&pixels, (256, 96)),
            ])
        },
        move |studio, app, _, result| {
            *MEASURING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            match result {
                Ok([waveform, vectorscope, histogram]) => {
                    let global = app.global::<ForkScopes>();
                    global.set_waveform(image(&waveform));
                    global.set_vectorscope(image(&vectorscope));
                    global.set_histogram(image(&histogram));
                    global.set_ready(true);
                }
                Err(error) => log::debug!("scopes: {error}"),
            }
            if AGAIN.swap(false, Ordering::AcqRel) {
                after_frame(studio, f64::from(studio.playhead));
            }
        },
    );
}
