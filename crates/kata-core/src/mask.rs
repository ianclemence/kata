//! The alpha mask and the operations a person can apply to it.
//!
//! A mask is one byte per pixel, 0 fully transparent to 255 fully opaque. It is
//! the same resolution as the image it belongs to; nothing here rescales
//! implicitly.

use crate::error::{KataError, Result};
use crate::parallel::for_each_row_mut;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// The mask-refine panel, one struct so the UI, the CLI and the app all pass
/// the same thing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskParams {
    /// Raw mask values at or above this become opaque.
    pub threshold: u8,
    /// Softness of the final edge. 0 is a hard edge; 100 is about 4 px of blur.
    pub feather: u8,
    /// Positive dilates the subject, negative erodes it, in whole pixels.
    pub erode_dilate: i32,
    /// Fill interior holes: eyes between an arm and a torso, gaps in hair.
    pub fill_holes: bool,
    /// Keep only the biggest connected blob, dropping stray background pieces.
    pub keep_largest: bool,
    /// Replace the colour of semi-transparent edge pixels with the colour of
    /// nearby solid pixels, so no background colour fringes the cutout.
    pub despill: bool,
    /// Nudge the whole mask, in pixels.
    pub shift: (i32, i32),
}

impl Default for MaskParams {
    fn default() -> Self {
        MaskParams {
            threshold: 128,
            feather: 0,
            erode_dilate: 0,
            fill_holes: true,
            keep_largest: true,
            despill: true,
            shift: (0, 0),
        }
    }
}

impl Mask {
    pub fn new(width: u32, height: u32, value: u8) -> Self {
        Mask {
            width,
            height,
            data: vec![value; width as usize * height as usize],
        }
    }

    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<Self> {
        let expected = width as usize * height as usize;
        if data.len() != expected {
            return Err(KataError::Invalid(format!(
                "mask has {} bytes but {width}x{height} needs {expected}",
                data.len()
            )));
        }
        Ok(Mask {
            width,
            height,
            data,
        })
    }

    /// Take the alpha channel of an image as a mask.
    pub fn from_alpha(image: &RgbaImage) -> Self {
        let (width, height) = image.dimensions();
        let mut data = vec![0u8; width as usize * height as usize];
        for (index, pixel) in image.pixels().enumerate() {
            data[index] = pixel.0[3];
        }
        Mask {
            width,
            height,
            data,
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.data[y as usize * self.width as usize + x as usize]
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, value: u8) {
        self.data[y as usize * self.width as usize + x as usize] = value;
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|&v| v == 0)
    }

    /// Fraction of pixels that are at least half opaque.
    pub fn coverage(&self) -> f32 {
        let solid = self.data.iter().filter(|&&v| v >= 128).count();
        solid as f32 / self.data.len().max(1) as f32
    }

    /// Apply the whole refine panel in the order that makes the controls feel
    /// independent: decide what is subject, clean up the choice, then soften
    /// the edge last so the other controls do not fight the feather.
    pub fn refine(&self, params: &MaskParams) -> Mask {
        let mut out = self.clone();
        if params.threshold > 0 || params.keep_largest || params.fill_holes {
            out = out.thresholded(params.threshold);
        }
        if params.keep_largest {
            out.keep_largest_in_place();
        }
        if params.fill_holes {
            out.fill_holes_in_place();
        }
        if params.erode_dilate > 0 {
            out = out.dilate(params.erode_dilate as u32);
        } else if params.erode_dilate < 0 {
            out = out.erode((-params.erode_dilate) as u32);
        }
        if params.shift != (0, 0) {
            out = out.shifted(params.shift.0, params.shift.1);
        }
        if params.feather > 0 {
            out = out.blur(params.feather as f32 / 25.0);
        }
        out
    }

    /// Hard cut at `level`.
    pub fn thresholded(&self, level: u8) -> Mask {
        let mut out = self.clone();
        for value in &mut out.data {
            *value = if *value >= level { 255 } else { 0 };
        }
        out
    }

    pub fn inverted(&self) -> Mask {
        let mut out = self.clone();
        for value in &mut out.data {
            *value = 255 - *value;
        }
        out
    }

    /// Separable Gaussian blur on the mask, edge-clamped.
    pub fn blur(&self, sigma: f32) -> Mask {
        if sigma <= 0.01 {
            return self.clone();
        }
        let radius = (sigma * 3.0).ceil() as i32;
        let kernel = gaussian_kernel(sigma, radius);
        let (w, h) = (self.width as usize, self.height as usize);

        // Horizontal pass into f32.
        let mut temp = vec![0f32; w * h];
        for_each_row_mut(&mut temp, w, |y, row| {
            let src = &self.data[y * w..(y + 1) * w];
            for (x, out) in row.iter_mut().enumerate() {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let offset = x as i32 + k as i32 - radius;
                    let sx = offset.clamp(0, w as i32 - 1) as usize;
                    sum += src[sx] as f32 * weight;
                }
                *out = sum;
            }
        });

        // Vertical pass back to bytes.
        let mut out = Mask::new(self.width, self.height, 0);
        for_each_row_mut(&mut out.data, w, |y, row| {
            for (x, value) in row.iter_mut().enumerate() {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let offset = y as i32 + k as i32 - radius;
                    let sy = offset.clamp(0, h as i32 - 1) as usize;
                    sum += temp[sy * w + x] * weight;
                }
                *value = sum.round().clamp(0.0, 255.0) as u8;
            }
        });
        out
    }

    pub fn dilate(&self, radius: u32) -> Mask {
        if radius == 0 {
            return self.clone();
        }
        morphology(self, radius as usize, true)
    }

    pub fn erode(&self, radius: u32) -> Mask {
        if radius == 0 {
            return self.clone();
        }
        morphology(self, radius as usize, false)
    }

    /// Fill holes that do not touch the border: anything the background cannot
    /// reach by flooding in from the edge is part of the subject.
    pub fn fill_holes_in_place(&mut self) {
        let (w, h) = (self.width as usize, self.height as usize);
        let source = &self.data;
        let mut outside = vec![false; w * h];
        let mut queue: Vec<usize> = Vec::new();

        for x in 0..w {
            seed(x, source, &mut outside, &mut queue);
            seed((h - 1) * w + x, source, &mut outside, &mut queue);
        }
        for y in 0..h {
            seed(y * w, source, &mut outside, &mut queue);
            seed(y * w + w - 1, source, &mut outside, &mut queue);
        }

        while let Some(index) = queue.pop() {
            let x = index % w;
            let y = index / w;
            if x > 0 {
                seed(index - 1, source, &mut outside, &mut queue);
            }
            if x + 1 < w {
                seed(index + 1, source, &mut outside, &mut queue);
            }
            if y > 0 {
                seed(index - w, source, &mut outside, &mut queue);
            }
            if y + 1 < h {
                seed(index + w, source, &mut outside, &mut queue);
            }
        }

        for (index, value) in self.data.iter_mut().enumerate() {
            if !outside[index] {
                *value = 255;
            }
        }
    }

    /// Keep the largest connected region of opaque pixels and drop the rest.
    pub fn keep_largest_in_place(&mut self) {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut labels = vec![0u32; w * h];
        let mut next_label = 0u32;
        let mut best = (0u32, 0usize);
        let mut stack: Vec<usize> = Vec::new();

        for start in 0..w * h {
            if self.data[start] < 128 || labels[start] != 0 {
                continue;
            }
            next_label += 1;
            let mut area = 0usize;
            labels[start] = next_label;
            stack.push(start);
            while let Some(index) = stack.pop() {
                area += 1;
                let x = index % w;
                let y = index / w;
                let neighbours = [
                    (x > 0).then(|| index - 1),
                    (x + 1 < w).then(|| index + 1),
                    (y > 0).then(|| index - w),
                    (y + 1 < h).then(|| index + w),
                ];
                for neighbour in neighbours.into_iter().flatten() {
                    if self.data[neighbour] >= 128 && labels[neighbour] == 0 {
                        labels[neighbour] = next_label;
                        stack.push(neighbour);
                    }
                }
            }
            if area > best.1 {
                best = (next_label, area);
            }
        }

        if best.0 == 0 {
            return;
        }
        for (index, value) in self.data.iter_mut().enumerate() {
            if labels[index] != best.0 {
                *value = 0;
            }
        }
    }

    /// Move the mask without wrapping; vacated pixels become transparent.
    pub fn shifted(&self, dx: i32, dy: i32) -> Mask {
        let (w, h) = (self.width as i32, self.height as i32);
        let mut out = Mask::new(self.width, self.height, 0);
        for y in 0..h {
            let sy = y - dy;
            if sy < 0 || sy >= h {
                continue;
            }
            for x in 0..w {
                let sx = x - dx;
                if sx < 0 || sx >= w {
                    continue;
                }
                out.data[(y * w + x) as usize] = self.data[(sy * w + sx) as usize];
            }
        }
        out
    }

    /// Nearest-neighbour resize. Masks are rescaled only to match an image that
    /// was already rescaled, never as part of a quality decision.
    pub fn resized_nearest(&self, width: u32, height: u32) -> Mask {
        let mut out = Mask::new(width, height, 0);
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 {
            return out;
        }
        for y in 0..height {
            let sy = (u64::from(y) * u64::from(self.height) / u64::from(height))
                .min(u64::from(self.height) - 1) as u32;
            for x in 0..width {
                let sx = (u64::from(x) * u64::from(self.width) / u64::from(width))
                    .min(u64::from(self.width) - 1) as u32;
                out.set(x, y, self.get(sx, sy));
            }
        }
        out
    }

    /// Bounding box of pixels that are at least half opaque.
    pub fn bbox(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) >= 128 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if min_x == u32::MAX {
            None
        } else {
            Some((min_x, min_y, max_x, max_y))
        }
    }

    /// Multiply two masks of equal size.
    pub fn intersect(&self, other: &Mask) -> Mask {
        let mut out = self.clone();
        for (index, value) in out.data.iter_mut().enumerate() {
            *value = ((u16::from(*value) * u16::from(other.data[index])) / 255) as u8;
        }
        out
    }
}

fn gaussian_kernel(sigma: f32, radius: i32) -> Vec<f32> {
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut total = 0.0f32;
    for i in -radius..=radius {
        let x = i as f32;
        let value = (-(x * x) / (2.0 * sigma * sigma)).exp();
        kernel.push(value);
        total += value;
    }
    for value in &mut kernel {
        *value /= total;
    }
    kernel
}

/// Square (L-infinity) morphology, separable into a horizontal and a vertical
/// pass, which is what makes a 20 px radius affordable.
fn morphology(mask: &Mask, radius: usize, dilate: bool) -> Mask {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let mut horizontal = vec![0u8; w * h];
    for_each_row_mut(&mut horizontal, w, |y, row| {
        let src = &mask.data[y * w..(y + 1) * w];
        for (x, out) in row.iter_mut().enumerate() {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(w - 1);
            // Erosion treats everything beyond the edge as background, so a
            // subject that runs to the border still shrinks. Dilation clamps,
            // because there is nothing outside to grow into.
            *out = if !dilate && (x < radius || x + radius >= w) {
                0
            } else {
                reduce(&src[lo..=hi], dilate)
            };
        }
    });

    let mut out = Mask::new(mask.width, mask.height, 0);
    for_each_row_mut(&mut out.data, w, |y, row| {
        for (x, value) in row.iter_mut().enumerate() {
            if !dilate && (y < radius || y + radius >= h) {
                *value = 0;
                continue;
            }
            let lo = y.saturating_sub(radius);
            let hi = (y + radius).min(h - 1);
            let mut acc = if dilate { 0u8 } else { 255u8 };
            for sy in lo..=hi {
                let v = horizontal[sy * w + x];
                acc = if dilate { acc.max(v) } else { acc.min(v) };
            }
            *value = acc;
        }
    });
    out
}

/// Mark a background pixel as reachable from the border.
fn seed(index: usize, source: &[u8], outside: &mut [bool], queue: &mut Vec<usize>) {
    if !outside[index] && source[index] < 128 {
        outside[index] = true;
        queue.push(index);
    }
}

fn reduce(values: &[u8], dilate: bool) -> u8 {
    if dilate {
        values.iter().copied().max().unwrap_or(0)
    } else {
        values.iter().copied().min().unwrap_or(255)
    }
}

/// Replace the colour of partially transparent edge pixels with the colour of
/// solid pixels nearby. Without this, a cutout on a red-wall background keeps a
/// red rim, which posterising then turns into a hard pink ink line.
pub fn despill(image: &mut RgbaImage, mask: &Mask) {
    let (w, h) = image.dimensions();
    if (w, h) != mask.dimensions() {
        return;
    }
    let source = image.clone();
    let pixels: &mut [u8] = image.as_mut();
    let stride = w as usize;

    for_each_row_mut(pixels, stride * 4, |y, row| {
        for (x, pixel) in row.chunks_mut(4).enumerate() {
            let alpha = mask.data[y * stride + x];
            if alpha == 0 || alpha == 255 {
                continue;
            }
            // Average the solid neighbours in a small window; fall back to the
            // pixel itself when there are none, so we never invent a colour.
            let mut counts = [0u32; 4];
            let radius = 3i32;
            for dy in -radius..=radius {
                let ny = y as i32 + dy;
                if ny < 0 || ny >= h as i32 {
                    continue;
                }
                for dx in -radius..=radius {
                    let nx = x as i32 + dx;
                    if nx < 0 || nx >= w as i32 {
                        continue;
                    }
                    let n_index = ny as usize * stride + nx as usize;
                    if mask.data[n_index] == 255 {
                        let p = source.get_pixel(nx as u32, ny as u32).0;
                        for c in 0..4 {
                            counts[c] += u32::from(p[c]);
                        }
                    }
                }
            }
            if counts[3] > 0 {
                // counts[3] is the sum of the 255 alphas; divide by 255 to get
                // the number of contributing pixels.
                let n = counts[3] / 255;
                for c in 0..3 {
                    if let Some(v) = counts[c].checked_div(n) {
                        pixel[c] = v.min(255) as u8;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(width: u32, height: u32) -> Mask {
        let mut mask = Mask::new(width, height, 0);
        for y in 0..height {
            for x in 0..width {
                if x < width / 2 {
                    mask.set(x, y, 255);
                }
            }
        }
        mask
    }

    #[test]
    fn threshold_cuts_hard() {
        let mut mask = Mask::new(3, 1, 0);
        mask.data = vec![0, 127, 128];
        let cut = mask.thresholded(128);
        assert_eq!(cut.data, vec![0, 0, 255]);
    }

    #[test]
    fn dilate_grows_and_erode_shrinks() {
        // Columns 0..=3 are solid out of 9, so the boundary sits at x = 3.
        let mask = checker(9, 5);
        let grown = mask.dilate(2);
        let shrunk = mask.erode(1);
        assert!(grown.coverage() > mask.coverage());
        assert!(shrunk.coverage() < mask.coverage());
        // A 2 px dilation moves the boundary two pixels right, and no further.
        assert_eq!(grown.get(5, 2), 255);
        assert_eq!(grown.get(6, 2), 0);
        // A 1 px erosion pulls the boundary in from each side.
        assert_eq!(shrunk.get(1, 2), 255);
        assert_eq!(shrunk.get(2, 2), 255);
        assert_eq!(shrunk.get(0, 2), 0);
        assert_eq!(shrunk.get(3, 2), 0);
    }

    #[test]
    fn erosion_never_underflows_the_border() {
        let mask = Mask::new(4, 4, 255);
        let eroded = mask.erode(10);
        assert!(eroded.is_empty());
    }

    #[test]
    fn fill_holes_keeps_the_interior_and_respects_the_border() {
        // A ring with a hole in the middle.
        let mut mask = Mask::new(9, 9, 255);
        for y in 3..6 {
            for x in 3..6 {
                mask.set(x, y, 0);
            }
        }
        let mut filled = mask.clone();
        filled.fill_holes_in_place();
        assert_eq!(filled.get(4, 4), 255);

        // A gap that reaches the border is background, not a hole.
        let mut open = Mask::new(9, 9, 255);
        for y in 0..9 {
            open.set(4, y, 0);
        }
        let mut filled_open = open.clone();
        filled_open.fill_holes_in_place();
        assert_eq!(filled_open.get(4, 0), 0);
        assert_eq!(filled_open.get(4, 8), 0);
    }

    #[test]
    fn keep_largest_drops_small_blobs() {
        let mut mask = Mask::new(20, 20, 0);
        for y in 0..10 {
            for x in 0..10 {
                mask.set(x, y, 255);
            }
        }
        mask.set(18, 18, 255);
        mask.keep_largest_in_place();
        assert_eq!(mask.get(5, 5), 255);
        assert_eq!(mask.get(18, 18), 0);
        assert_eq!(mask.coverage(), 0.25);
    }

    #[test]
    fn shift_moves_without_wrapping() {
        let mut mask = Mask::new(5, 5, 0);
        mask.set(0, 0, 255);
        let moved = mask.shifted(2, 1);
        assert_eq!(moved.get(2, 1), 255);
        assert_eq!(moved.get(0, 0), 0);
        let wrapped = mask.shifted(-1, 0);
        assert!(wrapped.is_empty());
    }

    #[test]
    fn blur_softens_a_hard_edge_and_conserves_area() {
        let mask = checker(64, 8);
        let blurred = mask.blur(2.0);
        // The edge is no longer binary.
        let mid: Vec<u8> = blurred
            .data
            .iter()
            .copied()
            .filter(|&v| v > 10 && v < 245)
            .collect();
        assert!(!mid.is_empty());
        let sum: f64 = blurred.data.iter().map(|&v| f64::from(v)).sum();
        let expected = 255.0 * 64.0 / 2.0 * 8.0;
        assert!((sum - expected).abs() / expected < 0.01);
    }

    #[test]
    fn bbox_reports_the_solid_region() {
        let mut mask = Mask::new(10, 10, 0);
        mask.set(2, 3, 255);
        mask.set(6, 8, 255);
        assert_eq!(mask.bbox(), Some((2, 3, 6, 8)));
        assert_eq!(Mask::new(4, 4, 0).bbox(), None);
    }

    #[test]
    fn refine_is_idempotent_for_neutral_params() {
        let mask = checker(16, 16);
        let params = MaskParams {
            threshold: 128,
            feather: 0,
            erode_dilate: 0,
            fill_holes: false,
            keep_largest: false,
            despill: false,
            shift: (0, 0),
        };
        assert_eq!(mask.refine(&params), mask);
    }
}
