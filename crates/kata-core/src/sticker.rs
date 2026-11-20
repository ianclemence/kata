//! Turning a photo and a mask into a finished sticker.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use crate::filters;
use crate::grain::{halftone, paper_grain};
use crate::halo;
use crate::mask::{despill, Mask, MaskParams};
use crate::posterize::{posterize, posterize_preview, PosterizeOptions};
use crate::style::{HaloStyle, StickerStyle};
use image::RgbaImage;

/// The cutout on its own: the photo with the mask as its alpha channel, edge
/// colours cleaned so no background fringe survives.
pub fn cutout(source: &RgbaImage, mask: &Mask, params: &MaskParams) -> Result<RgbaImage> {
    if source.dimensions() != mask.dimensions() {
        return Err(KataError::Invalid(format!(
            "the mask is {}x{} but the image is {}x{}",
            mask.width,
            mask.height,
            source.width(),
            source.height()
        )));
    }
    let refined = mask.refine(params);
    let mut image = source.clone();
    if params.despill {
        despill(&mut image, &refined);
    }
    let stride = image.width() as usize;
    for (index, pixel) in image.pixels_mut().enumerate() {
        pixel.0[3] = refined.data[index / stride * stride + index % stride];
    }
    Ok(image)
}

/// The full pipeline: refine the mask, posterise, add print artefacts, lay the
/// subject on its halo.
pub fn render_sticker(
    source: &RgbaImage,
    raw_mask: &Mask,
    params: &MaskParams,
    style: &StickerStyle,
) -> Result<RgbaImage> {
    style.validate()?;
    if source.dimensions() != raw_mask.dimensions() {
        return Err(KataError::Invalid(format!(
            "the mask is {}x{} but the image is {}x{}",
            raw_mask.width,
            raw_mask.height,
            source.width(),
            source.height()
        )));
    }

    let refined = raw_mask.refine(params);
    if refined.is_empty() {
        return Err(KataError::EmptySubject);
    }

    let inks = style.palette_inks()?;

    // The posteriser reads the subject's own brightness distribution, so it
    // needs the mask already applied.
    let mut layered = source.clone();
    {
        let width = layered.width() as usize;
        for (index, pixel) in layered.pixels_mut().enumerate() {
            pixel.0[3] = refined.data[(index / width) * width + index % width];
        }
    }
    if params.despill {
        despill(&mut layered, &refined);
    }

    let options = PosterizeOptions {
        palette: inks.clone(),
        smooth: style.smooth,
        clahe: style.clahe,
        highlight_clip: style.highlight_clip,
        median_radius: style.median_radius,
        working_side: 1600,
        registration: style.registration,
    };
    let mut printed = posterize(&layered, &options)?.image;

    if style.halftone > 0 {
        halftone(&mut printed, &inks, style.halftone, 6.0);
    }

    let long_side = source.width().max(source.height());
    let halo_mask = halo::halo_alpha(&refined, &style.halo, long_side);
    let mut out = halo::composite(&printed, &refined, &halo_mask, style.halo.color);

    if style.grain > 0 {
        paper_grain(&mut out, style.grain, 0x4B41_5441);
    }
    Ok(out)
}

/// A fast, lower-resolution render for the live preview panel. Same maths, same
/// look, a fraction of the pixels.
pub fn render_sticker_preview(
    source: &RgbaImage,
    raw_mask: &Mask,
    params: &MaskParams,
    style: &StickerStyle,
    long_side: u32,
) -> Result<RgbaImage> {
    let longest = source.width().max(source.height());
    if longest <= long_side {
        return render_sticker(source, raw_mask, params, style);
    }
    let scale = long_side as f32 / longest.max(1) as f32;
    let width = ((source.width() as f32 * scale).round() as u32).max(1);
    let height = ((source.height() as f32 * scale).round() as u32).max(1);
    let small = filters::resize(source, width, height);
    let small_mask = raw_mask.resized_nearest(width, height);

    // Keep the halo proportional to the preview, not to the original.
    let mut preview_style = style.clone();
    preview_style.halo.auto_scale = false;
    preview_style.halo.width =
        style.halo.width * width as f32 / source.width().max(1) as f32;

    render_sticker(&small, &small_mask, params, &preview_style)
}

/// Posterise only, for a preview that shows the print without the halo.
pub fn render_print_preview(
    source: &RgbaImage,
    mask: &Mask,
    params: &MaskParams,
    style: &StickerStyle,
    long_side: u32,
) -> Result<RgbaImage> {
    let refined = mask.refine(params);
    let mut layered = source.clone();
    let width = layered.width() as usize;
    for (index, pixel) in layered.pixels_mut().enumerate() {
        pixel.0[3] = refined.data[(index / width) * width + index % width];
    }
    let options = PosterizeOptions {
        palette: style.palette_inks()?,
        smooth: style.smooth,
        clahe: style.clahe,
        highlight_clip: style.highlight_clip,
        median_radius: style.median_radius,
        working_side: 1600,
        registration: style.registration,
    };
    Ok(posterize_preview(&layered, &options, long_side)?.image)
}

/// The halo on its own, so the UI can show what the width setting will produce.
pub fn preview_halo(width: u32, height: u32, style: &HaloStyle) -> Mask {
    let mut mask = Mask::new(width, height, 0);
    let cx = width / 2;
    let cy = height / 2;
    let radius = (width.min(height) / 4).max(1);
    for y in cy.saturating_sub(radius)..(cy + radius).min(height) {
        for x in cx.saturating_sub(radius)..(cx + radius).min(width) {
            let dx = x as i64 - cx as i64;
            let dy = y as i64 - cy as i64;
            if dx * dx + dy * dy <= (radius as i64) * (radius as i64) {
                mask.set(x, y, 255);
            }
        }
    }
    halo::halo_alpha(&mask, style, width.max(height))
}

/// Compose a finished sticker over a preview background, for the before/after
/// view. `background` may include alpha, which is how the checkerboard is drawn
/// without building an image for it.
pub fn over_background(sticker: &RgbaImage, background: Rgba) -> RgbaImage {
    halo::flatten(sticker, background)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba as Pixel;

    fn photo(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            // A bright subject block on a dark background.
            let inside = x > width / 4 && x < width * 3 / 4 && y > height / 4 && y < height * 3 / 4;
            let v = if inside { 210 } else { 40 };
            let tint = ((x + y) % 16) as u8;
            *pixel = Pixel([v, v.saturating_add(tint / 4), v.saturating_sub(tint / 4), 255]);
        }
        img
    }

    fn block_mask(width: u32, height: u32) -> Mask {
        let mut mask = Mask::new(width, height, 0);
        for y in height / 4..height * 3 / 4 {
            for x in width / 4..width * 3 / 4 {
                mask.set(x, y, 255);
            }
        }
        mask
    }

    #[test]
    fn cutout_applies_the_mask() {
        let img = photo(32, 32);
        let mask = block_mask(32, 32);
        let out = cutout(&img, &mask, &MaskParams::default()).unwrap();
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(16, 16).0[3], 255);
    }

    #[test]
    fn cutout_rejects_a_mismatched_mask() {
        let img = photo(32, 32);
        let mask = Mask::new(16, 16, 255);
        assert!(cutout(&img, &mask, &MaskParams::default()).is_err());
    }

    #[test]
    fn sticker_has_a_halo_colour_ring() {
        let img = photo(64, 64);
        let mask = block_mask(64, 64);
        let style = StickerStyle {
            inks: 3,
            halo: HaloStyle {
                width: 8.0,
                auto_scale: false,
                softness: 0,
                offset: (0, 0),
                ..Default::default()
            },
            grain: 0,
            ..Default::default()
        };
        let out = render_sticker(&img, &mask, &MaskParams::default(), &style).unwrap();
        assert_eq!(out.get_pixel(0, 0).0[3], 0, "the corner should be clear");
        // Just outside the subject block, the halo.
        let ring = out.get_pixel(64 / 4 - 4, 32).0;
        assert_eq!(ring[3], 255);
        assert_eq!(&ring[0..3], &[0xFB, 0xEF, 0xDC]);
    }

    #[test]
    fn an_empty_mask_is_reported_rather_than_exported() {
        let img = photo(32, 32);
        let mask = Mask::new(32, 32, 0);
        let err = render_sticker(&img, &mask, &MaskParams::default(), &StickerStyle::default());
        assert!(matches!(err, Err(KataError::EmptySubject)));
    }

    #[test]
    fn the_sticker_only_uses_palette_inks_plus_the_halo() {
        let img = photo(64, 64);
        let mask = block_mask(64, 64);
        let style = StickerStyle {
            inks: 3,
            grain: 0,
            halftone: 0,
            registration: 0,
            halo: HaloStyle {
                width: 6.0,
                auto_scale: false,
                softness: 0,
                offset: (0, 0),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = render_sticker(&img, &mask, &MaskParams::default(), &style).unwrap();
        let mut allowed: Vec<[u8; 4]> = style
            .palette_inks()
            .unwrap()
            .iter()
            .map(|c| c.to_array())
            .collect();
        allowed.push(style.halo.color.to_array());
        for pixel in out.pixels() {
            if pixel.0[3] == 0 {
                continue;
            }
            assert!(
                allowed.contains(&pixel.0),
                "unexpected colour {:?} in the print",
                pixel.0
            );
        }
    }

    #[test]
    fn preview_matches_the_full_render_in_shape() {
        let img = photo(200, 120);
        let mask = block_mask(200, 120);
        let style = StickerStyle::default();
        let out = render_sticker_preview(&img, &mask, &MaskParams::default(), &style, 100).unwrap();
        assert!(out.width() <= 100);
        assert!(out.height() <= 100);
        assert_eq!(out.width() * 120, out.height() * 200, "aspect ratio drifted");
    }

    #[test]
    fn preview_of_a_small_image_is_the_full_render() {
        let img = photo(64, 64);
        let mask = block_mask(64, 64);
        let style = StickerStyle::default();
        let params = MaskParams::default();
        let a = render_sticker_preview(&img, &mask, &params, &style, 512).unwrap();
        let b = render_sticker(&img, &mask, &params, &style).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn halo_preview_is_a_ring() {
        let mask = preview_halo(64, 64, &HaloStyle::default());
        assert!(!mask.is_empty());
        assert_eq!(mask.get(0, 0), 0);
    }
}
