//! Edge refinement guided by the photo (He, Sun and Tang's guided filter).
//!
//! A segmentation mask knows where the subject is; the photo knows where its
//! edges are. The guided filter makes the mask follow the photo's edges inside
//! a small window, which recovers hair strands and soft fabric that a threshold
//! would chop off.

use crate::error::{KataError, Result};
use crate::mask::Mask;
use image::RgbaImage;

/// Box mean over a (2r+1)^2 window using a summed-area table. Edges use the
/// pixels that exist, so the border is not darkened.
fn box_mean(data: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut sat = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0f64;
        for x in 0..w {
            row += f64::from(data[y * w + x]);
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let sum = sat[y1 * (w + 1) + x1] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0]
                + sat[y0 * (w + 1) + x0];
            out[y * w + x] = (sum / ((x1 - x0) * (y1 - y0)) as f64) as f32;
        }
    }
    out
}

/// Refine `mask` using `image` as the guide. `radius` is the window half-size in
/// pixels; `epsilon` is the regulariser (small keeps more edge detail).
pub fn guided(mask: &Mask, image: &RgbaImage, radius: u32, epsilon: f32) -> Result<Mask> {
    if image.dimensions() != mask.dimensions() {
        return Err(KataError::Invalid(format!(
            "the matte guide is {:?} but the mask is {:?}",
            image.dimensions(),
            mask.dimensions()
        )));
    }
    let (w, h) = (mask.width as usize, mask.height as usize);
    let r = radius.clamp(1, 64) as usize;
    let guide: Vec<f32> = image
        .pixels()
        .map(|p| {
            (0.2126 * f32::from(p.0[0]) + 0.7152 * f32::from(p.0[1]) + 0.0722 * f32::from(p.0[2]))
                / 255.0
        })
        .collect();
    let p: Vec<f32> = mask.data.iter().map(|v| f32::from(*v) / 255.0).collect();

    let mean_i = box_mean(&guide, w, h, r);
    let mean_p = box_mean(&p, w, h, r);
    let ii: Vec<f32> = guide.iter().map(|v| v * v).collect();
    let ip: Vec<f32> = guide.iter().zip(&p).map(|(a, b)| a * b).collect();
    let corr_ii = box_mean(&ii, w, h, r);
    let corr_ip = box_mean(&ip, w, h, r);

    let mut a = vec![0f32; w * h];
    let mut b = vec![0f32; w * h];
    for i in 0..w * h {
        let var_i = corr_ii[i] - mean_i[i] * mean_i[i];
        let cov_ip = corr_ip[i] - mean_i[i] * mean_p[i];
        a[i] = cov_ip / (var_i + epsilon.max(1e-8));
        b[i] = mean_p[i] - a[i] * mean_i[i];
    }
    let mean_a = box_mean(&a, w, h, r);
    let mean_b = box_mean(&b, w, h, r);
    let data = (0..w * h)
        .map(|i| ((mean_a[i] * guide[i] + mean_b[i]).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    Ok(Mask {
        width: mask.width,
        height: mask.height,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_that_already_follows_the_edge_is_kept() {
        let mut img = RgbaImage::new(40, 40);
        let mut mask = Mask::new(40, 40, 0);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let inside = x < 20;
            *p = if inside {
                image::Rgba([10, 10, 10, 255])
            } else {
                image::Rgba([240, 240, 240, 255])
            };
            if inside {
                mask.set(x, y, 255);
            }
        }
        let out = guided(&mask, &img, 4, 1e-4).unwrap();
        assert!(out.get(5, 20) > 240, "{}", out.get(5, 20));
        assert!(out.get(35, 20) < 15, "{}", out.get(35, 20));
    }

    #[test]
    fn a_sloppy_edge_snaps_to_the_photo_edge() {
        // The photo's edge is at x=20; the mask's is two pixels off at x=22.
        let mut img = RgbaImage::new(40, 40);
        let mut mask = Mask::new(40, 40, 0);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = if x < 20 {
                image::Rgba([10, 10, 10, 255])
            } else {
                image::Rgba([240, 240, 240, 255])
            };
            if x < 22 {
                mask.set(x, y, 255);
            }
        }
        let out = guided(&mask, &img, 6, 1e-3).unwrap();
        assert!(
            out.get(21, 20) < out.get(19, 20),
            "{} {}",
            out.get(21, 20),
            out.get(19, 20)
        );
    }

    #[test]
    fn mismatched_sizes_are_refused() {
        assert!(guided(&Mask::new(4, 4, 0), &RgbaImage::new(5, 4), 2, 1e-4).is_err());
    }
}
