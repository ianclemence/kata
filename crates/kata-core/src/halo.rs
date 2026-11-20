//! The cream outline that turns a cutout into a sticker.
//!
//! The outline is the cutout grown outward and nudged off-register, so it looks
//! hand-cut rather than traced. It is drawn *behind* the subject, which means a
//! sticker keeps a clean edge over any background it is pasted onto.

use crate::color::{over, Rgba};
use crate::mask::Mask;
use crate::style::HaloStyle;
use image::RgbaImage;

/// Build the alpha of the halo for a subject mask.
///
/// The shape is `dilate(shift(mask))`, then softened. Shifting before growing
/// rather than after is what produces the hand-cut look: the outline is heavier
/// on two sides.
pub fn halo_alpha(mask: &Mask, style: &HaloStyle, long_side: u32) -> Mask {
    let width = style.effective_width(long_side);
    if width == 0 {
        return Mask::new(mask.width, mask.height, 0);
    }
    let shifted = if style.offset == (0, 0) {
        mask.clone()
    } else {
        mask.shifted(style.offset.0, style.offset.1)
    };
    let grown = shifted.dilate(width);
    if style.softness == 0 {
        grown
    } else {
        grown.blur(u32::from(style.softness.min(100)) as f32 / 25.0)
    }
}

/// Put the subject on top of its halo.
pub fn composite(subject: &RgbaImage, alpha: &Mask, halo: &Mask, color: Rgba) -> RgbaImage {
    let (width, height) = subject.dimensions();
    let mut out = RgbaImage::new(width, height);
    let cream = color.with_alpha(255);
    let stride = width as usize;

    for (index, pixel) in out.pixels_mut().enumerate() {
        let y = index / stride;
        let x = index % stride;
        let halo_value = halo.data[y * stride + x];
        let subject_pixel = subject.get_pixel(x as u32, y as u32);
        let mut backdrop = Rgba::TRANSPARENT;
        if halo_value > 0 {
            backdrop = cream.with_alpha(halo_value);
        }
        let mut front = Rgba::from(*subject_pixel);        front.a = alpha.data[y * stride + x];
        *pixel = image::Rgba(over(backdrop, front).to_array());
    }
    out
}

/// Flatten a sticker onto a solid colour, for exports that want no alpha.
pub fn flatten(image: &RgbaImage, background: Rgba) -> RgbaImage {
    let (width, height) = image.dimensions();
    let mut out = RgbaImage::new(width, height);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let front = Rgba::from(*image.get_pixel(x, y));
        *pixel = image::Rgba(over(background.with_alpha(255), front).to_array());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba as Pixel;

    fn subject(width: u32, height: u32) -> (RgbaImage, Mask) {
        let mut img = RgbaImage::new(width, height);
        let mut mask = Mask::new(width, height, 0);
        for y in height / 2 - 2..height / 2 + 2 {
            for x in width / 2 - 2..width / 2 + 2 {
                img.put_pixel(x, y, Pixel([20, 20, 20, 255]));
                mask.set(x, y, 255);
            }
        }
        (img, mask)
    }

    #[test]
    fn halo_grows_beyond_the_subject() {
        let (_, mask) = subject(64, 64);
        let style = HaloStyle {
            width: 16.0,
            auto_scale: false,
            softness: 0,
            offset: (0, 0),
            ..Default::default()
        };
        let halo = halo_alpha(&mask, &style, 64);
        assert_eq!(halo.get(32, 32 - 8), 255, "the halo did not grow upward");
        assert_eq!(halo.get(32, 0), 0, "the halo reached the border");
        assert!(halo.coverage() > mask.coverage());
    }

    #[test]
    fn zero_width_halo_is_empty() {
        let (_, mask) = subject(32, 32);
        let style = HaloStyle {
            width: 0.0,
            ..Default::default()
        };
        assert!(halo_alpha(&mask, &style, 500).is_empty());
    }

    #[test]
    fn the_offset_shifts_the_outline() {
        let (_, mask) = subject(64, 64);
        let style = HaloStyle {
            width: 6.0,
            auto_scale: false,
            softness: 0,
            offset: (6, 0),
            ..Default::default()
        };
        let halo = halo_alpha(&mask, &style, 64);
        let right = halo.get(32 + 8, 32);
        let left = halo.get(32 - 8, 32);
        assert!(right >= left, "the halo was not pushed to the right");
    }

    #[test]
    fn composite_draws_cream_outside_and_subject_inside() {
        let (img, mask) = subject(64, 64);
        let style = HaloStyle {
            width: 12.0,
            auto_scale: false,
            softness: 0,
            offset: (0, 0),
            ..Default::default()
        };
        let halo = halo_alpha(&mask, &style, 64);
        let out = composite(&img, &mask, &halo, crate::style::DEFAULT_HALO);
        assert_eq!(out.get_pixel(32, 32).0, [20, 20, 20, 255], "subject lost");
        let ring = out.get_pixel(32, 32 - 6).0;
        assert_eq!(ring[3], 255, "the halo is transparent");
        assert_eq!(&ring[0..3], &[0xFB, 0xEF, 0xDC], "wrong halo colour");
        assert_eq!(out.get_pixel(0, 0).0[3], 0, "the corner should be clear");
    }

    #[test]
    fn flatten_fills_the_background() {
        let mut img = RgbaImage::new(4, 4);
        img.put_pixel(1, 1, Pixel([1, 2, 3, 255]));
        let out = flatten(&img, Rgba::rgb(250, 240, 220));
        assert_eq!(out.get_pixel(0, 0).0, [250, 240, 220, 255]);
        assert_eq!(out.get_pixel(1, 1).0, [1, 2, 3, 255]);
    }
}
