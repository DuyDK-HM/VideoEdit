// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: motion tracking. Two points on a video are followed frame to
//! frame by template matching (normalised cross-correlation on a small
//! grey copy), and the pair's motion - its midpoint's move, its length's
//! change and its angle's turn - becomes position, scale and rotation for
//! whatever should stick to it.
//!
//! Pure Rust on the CPU at a few hundred pixels across: a ten-second clip
//! tracks in seconds, with no model to download. Each frame is matched
//! against the patch from the frame before, so gradual turns and zooms
//! are followed; the price is drift over long, featureless stretches.

use std::sync::atomic::{AtomicBool, Ordering};

use concat_core::frame::Frame;
use concat_core::time::Rational;
use concat_media::{DecodeOptions, Decoder, FrameSource};

/// Width the video is tracked at.
const WIDTH: u32 = 480;
/// Half the side of the patch followed, in tracking pixels.
const PATCH: i32 = 10;
/// How far a point may move between two sampled frames, in tracking pixels.
const SEARCH: i32 = 24;

/// A grey picture, one value a pixel.
struct Grey {
    width: i32,
    height: i32,
    pixels: Vec<f32>,
}

impl Grey {
    fn of(frame: &Frame) -> Self {
        let pixels = frame
            .pixels()
            .chunks_exact(4)
            .map(|p| 0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]))
            .collect();
        Self { width: frame.width() as i32, height: frame.height() as i32, pixels }
    }

    fn at(&self, x: i32, y: i32) -> f32 {
        let (x, y) = (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1));
        self.pixels[(y * self.width + x) as usize]
    }

    /// The patch around (x, y), mean taken out, and its spread.
    fn patch(&self, x: i32, y: i32) -> (Vec<f32>, f32) {
        let mut values = Vec::with_capacity(((2 * PATCH + 1) * (2 * PATCH + 1)) as usize);
        for dy in -PATCH..=PATCH {
            for dx in -PATCH..=PATCH {
                values.push(self.at(x + dx, y + dy));
            }
        }
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        values.iter_mut().for_each(|value| *value -= mean);
        let spread = values.iter().map(|value| value * value).sum::<f32>().sqrt();
        (values, spread)
    }
}

/// Where the patch of `before` at `point` is in `after`: the best
/// normalised cross-correlation within the search window.
fn follow(before: &Grey, after: &Grey, point: (i32, i32)) -> (i32, i32) {
    let (template, spread) = before.patch(point.0, point.1);
    if spread < 1e-3 {
        return point; // flat: nothing to follow
    }
    let mut best = (f32::MIN, point);
    for dy in -SEARCH..=SEARCH {
        for dx in -SEARCH..=SEARCH {
            let candidate = (point.0 + dx, point.1 + dy);
            let (values, other) = after.patch(candidate.0, candidate.1);
            if other < 1e-3 {
                continue;
            }
            let score = template.iter().zip(&values).map(|(a, b)| a * b).sum::<f32>() / (spread * other);
            if score > best.0 {
                best = (score, candidate);
            }
        }
    }
    best.1
}

/// Where the two points are at one moment, as fractions of the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Seconds after the tracking started.
    pub at: f64,
    /// The first point.
    pub a: (f64, f64),
    /// The second point.
    pub b: (f64, f64),
}

/// Follows points `a` and `b` (fractions of the picture) through `path`
/// from `from` for `length` seconds, sampling `per_second` frames a
/// second. Stops early, with what it has, when `cancel` is set.
pub fn track(
    path: &str,
    from: f64,
    length: f64,
    a: (f64, f64),
    b: (f64, f64),
    per_second: f64,
    cancel: &AtomicBool,
) -> Result<Vec<Sample>, String> {
    let info = concat_media::probe(path).map_err(crate::media::describe)?;
    let video = info.require_video().map_err(crate::media::describe)?;
    let fps = video.frame_rate.fps().as_f64().max(1.0);
    let height = ((f64::from(WIDTH) * f64::from(video.height) / f64::from(video.width)).round() as u32).max(2) & !1;
    let options = DecodeOptions::default()
        .scaled_to(WIDTH, height)
        .starting_at(Rational::approximate(from.max(0.0)).unwrap_or(Rational::ZERO));
    let mut decoder = Decoder::open(path, &options).map_err(crate::media::describe)?;
    let every = (fps / per_second.max(1.0)).round().max(1.0) as usize;
    let to_px = |(x, y): (f64, f64)| ((x * f64::from(WIDTH)) as i32, (y * f64::from(height)) as i32);
    let to_fraction = |(x, y): (i32, i32)| (f64::from(x) / f64::from(WIDTH), f64::from(y) / f64::from(height));

    let mut points = (to_px(a), to_px(b));
    let mut samples = vec![Sample { at: 0.0, a, b }];
    let mut before: Option<Grey> = None;
    let mut index = 0usize;
    while let Some(frame) = decoder.next_frame().map_err(crate::media::describe)? {
        let at = index as f64 / fps;
        index += 1;
        if at > length || cancel.load(Ordering::Relaxed) {
            break;
        }
        if !(index - 1).is_multiple_of(every) {
            continue;
        }
        let grey = Grey::of(&frame);
        if let Some(before) = &before {
            points = (follow(before, &grey, points.0), follow(before, &grey, points.1));
            samples.push(Sample { at, a: to_fraction(points.0), b: to_fraction(points.1) });
        }
        before = Some(grey);
    }
    Ok(samples)
}

/// How the pair moved from `first` to `now`: the midpoint's move (in
/// fractions of the picture), the length's ratio and the angle's turn in
/// degrees, clockwise.
pub fn motion(first: &Sample, now: &Sample) -> ((f64, f64), f64, f64) {
    let mid = |s: &Sample| ((s.a.0 + s.b.0) / 2.0, (s.a.1 + s.b.1) / 2.0);
    let span = |s: &Sample| (s.b.0 - s.a.0, s.b.1 - s.a.1);
    let ((x0, y0), (x1, y1)) = (mid(first), mid(now));
    let ((u0, v0), (u1, v1)) = (span(first), span(now));
    let (l0, l1) = (u0.hypot(v0), u1.hypot(v1));
    let scale = if l0 > 1e-9 { l1 / l0 } else { 1.0 };
    // Picture y runs down, so atan2's turn is already clockwise on screen.
    let turn = (v1.atan2(u1) - v0.atan2(u0)).to_degrees();
    let turn = (turn + 540.0) % 360.0 - 180.0;
    ((x1 - x0, y1 - y0), scale, turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, square: (i32, i32)) -> Grey {
        let mut frame = Frame::black(width, height);
        frame.fill([20, 20, 20, 255]);
        let mut grey = Grey::of(&frame);
        for y in square.1..square.1 + 12 {
            for x in square.0..square.0 + 12 {
                // A textured square: corners and gradient for the match to hold on.
                grey.pixels[(y * width as i32 + x) as usize] = 120.0 + ((x * 7 + y * 13) % 90) as f32;
            }
        }
        grey
    }

    #[test]
    fn a_patch_is_found_where_it_moved() {
        let before = picture(160, 120, (50, 40));
        let after = picture(160, 120, (57, 35));
        assert_eq!(follow(&before, &after, (56, 46)), (63, 41));
    }

    #[test]
    fn the_pair_gives_move_scale_and_turn() {
        let first = Sample { at: 0.0, a: (0.4, 0.5), b: (0.6, 0.5) };
        // Moved right by 0.1, twice as long, turned a quarter clockwise.
        let now = Sample { at: 1.0, a: (0.6, 0.3), b: (0.6, 0.7) };
        let ((dx, dy), scale, turn) = motion(&first, &now);
        assert!((dx - 0.1).abs() < 1e-9 && dy.abs() < 1e-9, "{dx} {dy}");
        assert!((scale - 2.0).abs() < 1e-9);
        assert!((turn - 90.0).abs() < 1e-9, "{turn}");
    }
}
