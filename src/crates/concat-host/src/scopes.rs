// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat fork contributors

//! Fork: colour scopes of a picture - luma waveform, vectorscope and RGB
//! histogram - drawn as small RGBA images for the grading panel.
//!
//! Computed on the CPU from a small copy of the frame (a few hundred
//! pixels across is what a scope can show anyway), so they cost the
//! preview nothing it was doing before.

/// A drawn scope: `width` by `height`, RGBA8, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Scope {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Scope {
    fn blank(width: u32, height: u32) -> Self {
        let mut pixels = vec![0; (width * height * 4) as usize];
        pixels.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&[12, 12, 14, 255]));
        Self { width, height, pixels }
    }

    /// Brightens one pixel by `amount` of `colour`, saturating.
    fn add(&mut self, x: u32, y: u32, colour: [u8; 3], amount: f32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = ((y * self.width + x) * 4) as usize;
        for (channel, &tint) in colour.iter().enumerate() {
            let value = f32::from(self.pixels[at + channel]) + f32::from(tint) * amount;
            self.pixels[at + channel] = value.min(255.0) as u8;
        }
    }
}

/// Rec. 709 luma of an RGB pixel, 0..1.
fn luma(p: &[u8]) -> f32 {
    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0
}

/// Luma waveform: across, the picture's columns; up, brightness.
pub fn waveform(rgba: &[u8], width: u32, height: u32, out: (u32, u32)) -> Scope {
    let mut scope = Scope::blank(out.0, out.1);
    let gain = 2.0 * out.1 as f32 / height.max(1) as f32;
    for (index, p) in rgba.chunks_exact(4).enumerate() {
        let x = (index as u32 % width) * out.0 / width.max(1);
        let y = ((1.0 - luma(p)) * (out.1 - 1) as f32).round() as u32;
        scope.add(x, y, [90, 230, 120], gain.min(1.0) * 0.6);
    }
    scope
}

/// Vectorscope: each pixel's chroma (Cb across, Cr up) around the centre.
pub fn vectorscope(rgba: &[u8], side: u32) -> Scope {
    let mut scope = Scope::blank(side, side);
    let half = side as f32 / 2.0;
    let count = (rgba.len() / 4).max(1) as f32;
    let amount = (side as f32 * side as f32 / count * 0.8).clamp(0.05, 1.0);
    for p in rgba.chunks_exact(4) {
        let (r, g, b) = (f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0);
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let (cb, cr) = ((b - y) / 1.8556, (r - y) / 1.5748);
        let x = (half + cb * half * 1.8) as u32;
        let y = (half - cr * half * 1.8) as u32;
        scope.add(x, y, [p[0].max(60), p[1].max(60), p[2].max(60)], amount);
    }
    // The skin-tone line, where faces land: 123 degrees from Cb.
    let angle = 123f32.to_radians();
    for step in 0..side / 2 {
        let r = step as f32;
        scope.add((half + r * angle.cos()) as u32, (half - r * angle.sin()) as u32, [80, 60, 40], 1.0);
    }
    scope
}

/// RGB histogram: across, value 0..255; up, how many pixels, per channel.
pub fn histogram(rgba: &[u8], out: (u32, u32)) -> Scope {
    let mut counts = [[0u32; 256]; 3];
    for p in rgba.chunks_exact(4) {
        for (count, &value) in counts.iter_mut().zip(p) {
            count[value as usize] += 1;
        }
    }
    let peak = counts.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
    let mut scope = Scope::blank(out.0, out.1);
    let colours = [[230, 70, 70], [70, 220, 90], [80, 120, 240]];
    for x in 0..out.0 {
        let bin = (x * 256 / out.0.max(1)) as usize;
        for (channel, colour) in colours.iter().enumerate() {
            // Square root, so the shadows of a bright frame still show.
            let tall = ((counts[channel][bin] as f32 / peak).sqrt() * out.1 as f32) as u32;
            for y in out.1.saturating_sub(tall)..out.1 {
                scope.add(x, y, *colour, 0.55);
            }
        }
    }
    scope
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(colour: [u8; 3], pixels: usize) -> Vec<u8> {
        (0..pixels).flat_map(|_| [colour[0], colour[1], colour[2], 255]).collect()
    }

    fn lit_rows(scope: &Scope) -> Vec<u32> {
        (0..scope.height)
            .filter(|y| {
                (0..scope.width).any(|x| scope.pixels[((y * scope.width + x) * 4 + 1) as usize] > 40)
            })
            .collect()
    }

    #[test]
    fn white_sits_at_the_top_of_the_waveform_and_black_at_the_bottom() {
        assert_eq!(lit_rows(&waveform(&solid([255, 255, 255], 64), 8, 8, (16, 32))), [0]);
        assert_eq!(lit_rows(&waveform(&solid([0, 0, 0], 64), 8, 8, (16, 32))), [31]);
    }

    #[test]
    fn grey_is_in_the_middle_of_the_vectorscope() {
        let scope = vectorscope(&solid([128, 128, 128], 64), 65);
        let centre = ((32 * 65 + 32) * 4) as usize;
        assert!(scope.pixels[centre] > 40, "grey has no chroma");
    }

    #[test]
    fn a_red_frame_fills_one_red_bar_at_the_right() {
        let scope = histogram(&solid([255, 0, 0], 100), (256, 50));
        let at = |x: u32, y: u32| ((y * 256 + x) * 4) as usize;
        assert!(scope.pixels[at(255, 0)] > 100, "red at 255, full height");
        assert!(scope.pixels[at(0, 0) + 1] > 100, "green at 0, full height");
        assert!(scope.pixels[at(128, 49) + 1] < 40, "nothing in the middle");
    }
}
