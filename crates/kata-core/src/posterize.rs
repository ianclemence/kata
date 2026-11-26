//! The signature effect: a photo, reduced to 2–5 flat inks that read like a
//! screen print.
//!
//! The order of operations matters and is the whole trick:
//! smooth (bilateral, so edges survive) → lift local contrast (CLAHE) → protect
//! the highlights → cut at percentiles → delete specks (median) → put the flat
//! regions back at full resolution.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use crate::filters;
use image::{GrayImage, Luma, RgbaImage};

#[derive(Clone, Debug, PartialEq)]
pub struct PosterizeOptions {
    /// Ink colours, ordered dark to light.
    pub palette: Vec<Rgba>,
    /// Bilateral strength 0..=100.
    pub smooth: u8,
    /// CLAHE clip strength 0..=100.
    pub clahe: u8,
    /// Fraction of the brightest pixels to blow out, 0.0..=0.2. Default 0.06.
    pub highlight_clip: f32,
    /// Median radius on the ink map, in pixels: 0, 1 or 2.
    pub median_radius: u32,
    /// Long side the expensive passes run at. The flat result is then brought
    /// back to full resolution, so this trades time for fine detail only.
    pub working_side: u32,
    /// Offset the darkest ink by this many pixels, 0..=3.
    pub registration: u8,
}

impl Default for PosterizeOptions {
    fn default() -> Self {
        PosterizeOptions {
            palette: vec![Rgba::BLACK, Rgba::WHITE],
            smooth: 60,
            clahe: 55,
            highlight_clip: 0.06,
            median_radius: 1,
            working_side: 1600,
            registration: 0,
        }
    }
}

/// A posterised image: flat ink colours at full resolution, plus the ink index
/// per pixel so later steps (halftone, misregistration) can reason about layers.
#[derive(Clone, Debug)]
pub struct Posterized {
    pub image: RgbaImage,
    pub index: GrayImage,
    pub palette: Vec<Rgba>,
}

impl Posterized {
    pub fn index_at(&self, x: u32, y: u32) -> u8 {
        self.index.get_pixel(x, y).0[0]
    }
}

pub fn posterize(source: &RgbaImage, options: &PosterizeOptions) -> Result<Posterized> {
    if options.palette.is_empty() {
        return Err(KataError::Invalid("the palette has no inks".into()));
    }
    if options.palette.len() > 5 {
        return Err(KataError::Invalid(format!(
            "{} inks were requested but screen printing uses at most 5",
            options.palette.len()
        )));
    }

    let (full_w, full_h) = source.dimensions();
    let working_side = options.working_side.max(64);
    let long_side = full_w.max(full_h);
    let working = if long_side > working_side {
        let scale = working_side as f32 / long_side as f32;
        let w = ((full_w as f32 * scale).round() as u32).max(1);
        let h = ((full_h as f32 * scale).round() as u32).max(1);
        filters::resize(source, w, h)
    } else {
        source.clone()
    };
    let (w, h) = working.dimensions();

    // 1. Edge-preserving smooth. Radius scales with the requested strength; a
    //    bigger radius at a fixed sigma simply costs more time.
    let smooth = options.smooth.min(100);
    let smoothed = if smooth == 0 {
        working.clone()
    } else {
        let radius = 1 + (u32::from(smooth) * 7) / 100;
        let sigma_color = 8.0 + f32::from(smooth) * 0.45;
        filters::bilateral(&working, radius, radius as f32 * 0.8, sigma_color)
    };

    // 2. Local contrast on lightness.
    let luma = filters::to_luma(&smoothed);
    let clahe_amount = options.clahe.min(100);
    let lifted = if clahe_amount == 0 {
        luma
    } else {
        let clip = 1.0 + f32::from(clahe_amount) * 0.06;
        filters::clahe(&luma, 8, clip)
    };

    // 3. Cut points from the subject only, so a large background cannot pull
    //    the inks towards its own brightness.
    let mut hist = [0u32; 256];
    let alpha = source.as_raw();
    let scale_x = full_w as f32 / w as f32;
    let scale_y = full_h as f32 / h as f32;
    for y in 0..h {
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * scale_x) as u32;
            let fy = ((y as f32 + 0.5) * scale_y) as u32;
            let fx = fx.min(full_w - 1);
            let fy = fy.min(full_h - 1);
            if alpha[((fy * full_w + fx) * 4 + 3) as usize] >= 128 {
                hist[lifted.get_pixel(x, y).0[0] as usize] += 1;
            }
        }
    }
    if hist.iter().all(|&c| c == 0) {
        // No opaque pixels at all: fall back to the whole plane.
        hist = filters::histogram(&lifted);
    }

    // 4. Clip the top highlights so faces sit below the paper white and keep
    //    their features, then stretch what is left back to the full range.
    let clip = options.highlight_clip.clamp(0.0, 0.2);
    let ceiling = if clip > 0.0 {
        filters::percentile(&hist, 1.0 - clip).max(1)
    } else {
        255
    };
    let stretch = 255.0 / f32::from(ceiling);

    let mut levels = vec![0u8; 256];
    for (value, out) in levels.iter_mut().enumerate() {
        let scaled = (value as f32 * stretch).round().clamp(0.0, 255.0) as u8;
        *out = scaled;
    }

    // Rebuild the histogram after the stretch so the cuts land on the final
    // values rather than the originals.
    let mut stretched_hist = [0u32; 256];
    for (value, &count) in hist.iter().enumerate() {
        stretched_hist[levels[value] as usize] += count;
    }

    let ink_count = options.palette.len();
    let mut cuts = Vec::with_capacity(ink_count.saturating_sub(1));
    for i in 1..ink_count {
        let fraction = i as f32 / ink_count as f32;
        cuts.push(filters::percentile(&stretched_hist, fraction));
    }

    // 5. Map to ink indices.
    let mut index = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let stretched = levels[lifted.get_pixel(x, y).0[0] as usize];
            let mut ink = 0u8;
            for &cut in &cuts {
                if stretched >= cut {
                    ink += 1;
                }
            }
            index.put_pixel(x, y, Luma([ink]));
        }
    }

    // 6. Median filter the ink map: a single misclassified pixel becomes a
    //    speck in a flat region, and specks are what make it look photographic.
    if options.median_radius > 0 {
        index = median(index, options.median_radius.min(2));
    }

    // 7. Back to full resolution. Upscaling the flat colours softly and then
    //    snapping each pixel to its nearest ink is what keeps the regions flat
    //    and the edges crisp at any size.
    let (index, palette) = if (w, h) != (full_w, full_h) {
        let small = to_rgba(&index, &options.palette);
        let big = filters::resize(&small, full_w, full_h);
        let (index, palette) = requantise(&big, &options.palette);
        (index, palette)
    } else {
        (index, options.palette.clone())
    };

    let mut image = to_rgba(&index, &palette);
    // Carry the original alpha through: the cutout is decided elsewhere.
    for y in 0..full_h {
        for x in 0..full_w {
            let a = source.get_pixel(x, y).0[3];
            image.get_pixel_mut(x, y).0[3] = a;
        }
    }

    if options.registration > 0 {
        misregister(&mut image, &index, &palette, options.registration.min(3));
    }

    Ok(Posterized {
        image,
        index,
        palette,
    })
}

/// Map an ink index plane to colours.
pub fn to_rgba(index: &GrayImage, palette: &[Rgba]) -> RgbaImage {
    let (w, h) = index.dimensions();
    let mut out = RgbaImage::new(w, h);
    let lookup: Vec<[u8; 4]> = palette.iter().map(|c| c.to_array()).collect();
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let i = index.get_pixel(x, y).0[0] as usize;
        *pixel = image::Rgba(lookup[i.min(lookup.len() - 1)]);
    }
    out
}

/// Snap every pixel of `image` to the nearest palette entry by lightness.
fn requantise(image: &RgbaImage, palette: &[Rgba]) -> (GrayImage, Vec<Rgba>) {
    let (w, h) = image.dimensions();
    let luma: Vec<u8> = palette.iter().map(Rgba::luma).collect();
    let mut index = GrayImage::new(w, h);
    for (x, y, pixel) in image.enumerate_pixels() {
        let p = pixel.0;
        let value =
            (2126 * u32::from(p[0]) + 7152 * u32::from(p[1]) + 722 * u32::from(p[2]) + 5000)
                / 10000;
        let mut best = 0usize;
        let mut best_distance = i32::MAX;
        for (i, &ink_luma) in luma.iter().enumerate() {
            let distance = (i32::from(ink_luma) - value as i32).abs();
            if distance < best_distance {
                best_distance = distance;
                best = i;
            }
        }
        index.put_pixel(x, y, Luma([best as u8]));
    }
    (index, palette.to_vec())
}

/// Median filter over the ink index plane.
fn median(index: GrayImage, radius: u32) -> GrayImage {
    let (w, h) = index.dimensions();
    let r = radius as i32;
    let mut out = GrayImage::new(w, h);
    let mut window: Vec<u8> = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
    for y in 0..h {
        for x in 0..w {
            window.clear();
            for dy in -r..=r {
                let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                for dx in -r..=r {
                    let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                    window.push(index.get_pixel(nx, ny).0[0]);
                }
            }
            window.sort_unstable();
            out.put_pixel(x, y, Luma([window[window.len() / 2]]));
        }
    }
    out
}

/// Ghost the darkest ink a pixel or two off register. Real screen prints miss by
/// a hair; the doubled edge is what makes a flat print look made by hand.
fn misregister(image: &mut RgbaImage, index: &GrayImage, palette: &[Rgba], offset: u8) {
    let (w, h) = index.dimensions();
    let dark = palette[0];
    let (dx, dy) = (offset as i32, (offset as i32 * 3) / 4);
    let source = image.clone();

    for y in 0..h {
        for x in 0..w {
            // The ghost appears where the dark ink is shifted onto a pixel that
            // the (unshifted) print leaves lighter.
            let sx = x as i32 - dx;
            let sy = y as i32 - dy;
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            let src_pixel = source.get_pixel(sx as u32, sy as u32);
            if src_pixel.0[3] == 0 {
                continue;
            }
            if index.get_pixel(sx as u32, sy as u32).0[0] != 0 {
                continue;
            }
            // Only where this pixel is not already dark itself.
            if index.get_pixel(x, y).0[0] == 0 {
                continue;
            }
            let pixel = image.get_pixel_mut(x, y);
            pixel.0[0] = dark.r;
            pixel.0[1] = dark.g;
            pixel.0[2] = dark.b;
        }
    }
}

/// Posterise a preview of at most `long_side` pixels, for the live panel.
pub fn posterize_preview(
    source: &RgbaImage,
    options: &PosterizeOptions,
    long_side: u32,
) -> Result<Posterized> {
    let (w, h) = source.dimensions();
    let longest = w.max(h);
    if longest <= long_side {
        return posterize(source, options);
    }
    let scale = long_side as f32 / longest as f32;
    let small = filters::resize(
        source,
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let mut adjusted = options.clone();
    adjusted.working_side = long_side;
    posterize(&small, &adjusted)
}

/// Count how many pixels carry each ink. Used by the UI to show a palette
/// swatch strip weighted by area, and by the tests to check flatness.
pub fn ink_area(index: &GrayImage, ink: u8) -> u32 {
    index.as_raw().iter().filter(|&&v| v == ink).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for (x, _, pixel) in img.enumerate_pixels_mut() {
            let v = (x * 255 / width.max(1)) as u8;
            *pixel = image::Rgba([v, v, v, 255]);
        }
        img
    }

    #[test]
    fn posterize_produces_exactly_the_palette_colours() {
        let palette = vec![Rgba::rgb(0, 0, 0), Rgba::rgb(128, 128, 128), Rgba::WHITE];
        let opts = PosterizeOptions {
            palette: palette.clone(),
            working_side: 128,
            ..Default::default()
        };
        let out = posterize(&gradient(64, 8), &opts).unwrap();
        let allowed: Vec<[u8; 4]> = palette.iter().map(|c| c.to_array()).collect();
        for pixel in out.image.pixels() {
            assert!(
                allowed.contains(&pixel.0),
                "found a colour that is not an ink: {:?}",
                pixel.0
            );
        }
    }

    #[test]
    fn posterize_uses_every_ink_on_a_gradient() {
        let palette = vec![
            Rgba::rgb(0, 0, 0),
            Rgba::rgb(64, 64, 64),
            Rgba::rgb(128, 128, 128),
            Rgba::rgb(192, 192, 192),
            Rgba::WHITE,
        ];
        let opts = PosterizeOptions {
            palette,
            smooth: 0,
            clahe: 0,
            highlight_clip: 0.0,
            working_side: 64,
            ..Default::default()
        };
        let out = posterize(&gradient(64, 4), &opts).unwrap();
        for ink in 0..5u8 {
            assert!(ink_area(&out.index, ink) > 0, "ink {ink} is unused");
        }
    }

    #[test]
    fn a_pixel_is_ranked_on_the_same_luma_scale_as_the_inks() {
        // Pure blue is Rec.709 luma 0.0722 * 255 ~ 18, so it belongs to an ink
        // of luma 30, not to black. `requantise` carried its own copy of the
        // Rec.709 weights with the blue term written 72 instead of 722, which
        // scores this pixel ~2 and drops it on the black ink, while the palette
        // beside it is measured with the correct weights. The two sides of the
        // comparison must not drift apart.
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 255, 255]));
        let palette = vec![Rgba::rgb(0, 0, 0), Rgba::rgb(30, 30, 30)];
        let (index, _) = requantise(&img, &palette);
        assert_eq!(index.get_pixel(0, 0).0[0], 1);
    }

    #[test]
    fn highlight_clip_maps_the_ceiling_to_white() {
        // A bright image: without clipping, nothing would be pure white.
        let mut img = RgbaImage::new(32, 4);
        for (_, _, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgba([200, 200, 200, 255]);
        }
        let opts = PosterizeOptions {
            palette: vec![Rgba::BLACK, Rgba::WHITE],
            smooth: 0,
            clahe: 0,
            highlight_clip: 0.06,
            working_side: 64,
            ..Default::default()
        };
        let out = posterize(&img, &opts).unwrap();
        assert!(ink_area(&out.index, 1) > 0, "nothing reached paper white");
    }

    #[test]
    fn rejects_more_than_five_inks() {
        let opts = PosterizeOptions {
            palette: vec![Rgba::BLACK; 6],
            ..Default::default()
        };
        assert!(posterize(&gradient(8, 8), &opts).is_err());
    }

    #[test]
    fn median_removes_isolated_specks() {
        let mut index = GrayImage::new(9, 9);
        for pixel in index.pixels_mut() {
            *pixel = Luma([1]);
        }
        index.put_pixel(4, 4, Luma([0]));
        let filtered = median(index, 1);
        assert_eq!(filtered.get_pixel(4, 4).0[0], 1);
    }

    #[test]
    fn registration_ghosts_the_dark_ink() {
        let palette = vec![Rgba::BLACK, Rgba::WHITE];
        let mut img = RgbaImage::new(8, 8);
        for (x, _, pixel) in img.enumerate_pixels_mut() {
            let v = if x < 4 { 0 } else { 255 };
            *pixel = image::Rgba([v, v, v, 255]);
        }
        let mut index = GrayImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                index.put_pixel(x, y, Luma([if x < 4 { 0 } else { 1 }]));
            }
        }
        misregister(&mut img, &index, &palette, 2);
        // A couple of pixels to the right of the edge is now dark.
        assert_eq!(img.get_pixel(5, 4).0[0], 0);
    }

    #[test]
    fn output_keeps_the_source_alpha() {
        let mut img = gradient(16, 4);
        for y in 0..4 {
            img.get_pixel_mut(0, y).0[3] = 0;
        }
        let opts = PosterizeOptions {
            working_side: 64,
            ..Default::default()
        };
        let out = posterize(&img, &opts).unwrap();
        assert_eq!(out.image.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.image.get_pixel(15, 0).0[3], 255);
    }
}
