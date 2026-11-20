//! Print artefacts: paper grain and halftone dots.
//!
//! Both are deterministic. A given pixel with a given seed always receives the
//! same offset, which is what lets the golden-image tests hold and what stops
//! the preview and the export disagreeing.

use crate::color::Rgba;
use image::RgbaImage;

/// A cheap, stable 2D hash. Not cryptographic; it only has to look like noise
/// and never change between runs or platforms.
fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ seed;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// Signed noise in -1.0..=1.0.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    (hash(x, y, seed) as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Add paper grain. `amount` is 0..=100. Only opaque-enough pixels are touched,
/// so a cutout keeps a clean transparent edge.
pub fn paper_grain(image: &mut RgbaImage, amount: u8, seed: u32) {
    let strength = f32::from(amount.min(100)) * 0.28;
    if strength <= 0.0 {
        return;
    }
    let (width, _) = image.dimensions();
    for (index, pixel) in image.pixels_mut().enumerate() {
        if pixel.0[3] < 8 {
            continue;
        }
        let x = (index as u32) % width;
        let y = (index as u32) / width;
        // Two octaves read as paper rather than as television static.
        let coarse = noise(x / 2, y / 2, seed);
        let fine = noise(x, y, seed ^ 0x5F35_6495);
        let delta = (coarse * 0.6 + fine * 0.4) * strength;
        // Slightly stronger in the midtones, the way ink sits in paper.
        let mid = 1.0 - ((f32::from(pixel.0[0]) / 255.0) - 0.5).abs() * 1.2;
        let delta = delta * mid.clamp(0.2, 1.0);
        for c in 0..3 {
            let v = f32::from(pixel.0[c]) + delta;
            pixel.0[c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Break a flat ink area into dots, the way a screen does.
///
/// The screen is a 45° grid. Pixels whose position falls inside a dot are kept
/// at their ink; the rest are lifted one step towards the light so the paper
/// shows through. `amount` is 0..=100.
pub fn halftone(image: &mut RgbaImage, palette: &[Rgba], amount: u8, cell: f32) {
    let strength = f32::from(amount.min(100)) / 100.0;
    if strength <= 0.0 || palette.len() < 2 {
        return;
    }
    let cell = cell.max(2.0);
    let (width, height) = image.dimensions();
    let color_of = |p: &image::Rgba<u8>| -> Option<usize> {
        palette.iter().position(|ink| ink.to_array() == p.0)
    };
    let cos = std::f32::consts::FRAC_1_SQRT_2;
    let sin = std::f32::consts::FRAC_1_SQRT_2;

    for y in 0..height {
        for x in 0..width {
            let pixel = image.get_pixel_mut(x, y);
            if pixel.0[3] < 8 {
                continue;
            }
            let Some(ink) = color_of(pixel) else {
                continue;
            };
            // The lightest ink is paper; there is nothing to break up.
            if ink + 1 >= palette.len() {
                continue;
            }
            let fx = x as f32;
            let fy = y as f32;
            let u = (fx * cos + fy * sin) / cell;
            let v = (-fx * sin + fy * cos) / cell;
            let du = u - u.floor() - 0.5;
            let dv = v - v.floor() - 0.5;
            let distance = (du * du + dv * dv).sqrt() * 2.0; // 0 at the centre
                                                             // Darker inks print bigger dots.
            let darkness = 1.0 - ink as f32 / (palette.len() - 1) as f32;
            let radius = (0.35 + darkness * 0.45) * strength;
            if distance > radius {
                let lighter = palette[(ink + 1).min(palette.len() - 1)];
                pixel.0[0] = lighter.r;
                pixel.0[1] = lighter.g;
                pixel.0[2] = lighter.b;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba as Pixel;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        for x in 0..50 {
            for y in 0..50 {
                let a = noise(x, y, 7);
                let b = noise(x, y, 7);
                assert_eq!(a, b);
                assert!((-1.0..=1.0).contains(&a));
            }
        }
    }

    #[test]
    fn grain_changes_pixels_but_not_alpha() {
        let mut img = RgbaImage::from_pixel(32, 32, Pixel([120, 120, 120, 255]));
        let before = img.clone();
        paper_grain(&mut img, 40, 1);
        assert_ne!(img, before);
        assert!(img.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn grain_leaves_transparent_pixels_alone() {
        let mut img = RgbaImage::new(8, 8);
        paper_grain(&mut img, 100, 1);
        assert!(img.pixels().all(|p| p.0 == [0, 0, 0, 0]));
    }

    #[test]
    fn grain_strength_scales_the_change() {
        let base = RgbaImage::from_pixel(64, 64, Pixel([128, 128, 128, 255]));
        let mut light = base.clone();
        paper_grain(&mut light, 10, 3);
        let mut heavy = base.clone();
        paper_grain(&mut heavy, 90, 3);
        let spread = |img: &RgbaImage| -> i32 {
            let min = img.pixels().map(|p| p.0[0]).min().unwrap() as i32;
            let max = img.pixels().map(|p| p.0[0]).max().unwrap() as i32;
            max - min
        };
        assert!(spread(&heavy) > spread(&light));
    }

    #[test]
    fn halftone_uses_only_palette_colours() {
        let palette = [Rgba::BLACK, Rgba::rgb(128, 128, 128), Rgba::WHITE];
        let mut img = RgbaImage::from_pixel(32, 32, Pixel([128, 128, 128, 255]));
        halftone(&mut img, &palette, 80, 4.0);
        let allowed: Vec<[u8; 4]> = palette.iter().map(|c| c.to_array()).collect();
        for pixel in img.pixels() {
            assert!(allowed.contains(&pixel.0), "{:?}", pixel.0);
        }
        // And it did something.
        assert!(img.pixels().any(|p| p.0 == Rgba::WHITE.to_array()));
    }

    #[test]
    fn halftone_is_a_no_op_at_zero() {
        let palette = [Rgba::BLACK, Rgba::WHITE];
        let mut img = RgbaImage::from_pixel(16, 16, Pixel([0, 0, 0, 255]));
        let before = img.clone();
        halftone(&mut img, &palette, 0, 4.0);
        assert_eq!(img, before);
    }

    #[test]
    fn halftone_leaves_the_lightest_ink_alone() {
        let palette = [Rgba::BLACK, Rgba::WHITE];
        let mut img = RgbaImage::from_pixel(16, 16, Pixel([255, 255, 255, 255]));
        let before = img.clone();
        halftone(&mut img, &palette, 100, 3.0);
        assert_eq!(img, before);
    }
}
