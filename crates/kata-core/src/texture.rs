//! Paper, made rather than scanned, and a check for scanned paper.
//!
//! A scanned paper texture brought fold creases into every poster laid over it.
//! Procedural paper cannot carry a crease; imported textures are checked for
//! long straight lines before use.

use image::GrayImage;
#[cfg(test)]
use image::Luma;
use serde::{Deserialize, Serialize};

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(tx), smooth(ty));
    let a = hash(x0, y0, seed);
    let b = hash(x0 + 1, y0, seed);
    let c = hash(x0, y0 + 1, seed);
    let d = hash(x0 + 1, y0 + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Grey paper texture centred on 128, for an overlay blend: cloudy fibre
/// density plus fine tooth. Deterministic for a given seed. No straight lines.
pub fn paper(width: u32, height: u32, seed: u32, strength: f32) -> GrayImage {
    let strength = strength.clamp(0.0, 1.0);
    let mut values = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32, y as f32);
            let mut cloud = 0.0;
            let mut amp = 1.0;
            let mut freq = 1.0 / 180.0;
            for octave in 0..4 {
                cloud += amp * (value_noise(fx * freq, fy * freq, seed.wrapping_add(octave)) - 0.5);
                amp *= 0.5;
                freq *= 2.0;
            }
            // Fibres: stretched noise at a slight angle, so tooth has a direction.
            let fibre = value_noise(fx * 0.9 + fy * 0.12, fy * 0.08, seed ^ 0x5a5a) - 0.5;
            let tooth = hash(x as i32, y as i32, seed ^ 0x1234) - 0.5;
            values.push(cloud * 70.0 + fibre * 26.0 + tooth * 18.0);
        }
    }
    // Centre exactly on 128 so an overlay blend neither darkens nor lightens overall.
    let mean = values.iter().sum::<f32>() / values.len().max(1) as f32;
    let data = values
        .iter()
        .map(|v| (128.0 + strength * (v - mean)).round().clamp(0.0, 255.0) as u8)
        .collect();
    GrayImage::from_raw(width, height, data).expect("width * height values")
}

/// A straight line found in a texture: a fold, a crease, a scanner edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crease {
    /// "vertical" or "horizontal" (within 15 degrees).
    pub orientation: String,
    /// Where the line crosses the image centre, in pixels from the left (or top).
    pub position: u32,
    pub angle_degrees: f32,
    /// How many standard deviations darker or lighter than other lines like it.
    pub strength: f32,
}

/// Find long straight lines that a texture would print onto every poster.
pub fn find_creases(texture: &GrayImage) -> Vec<Crease> {
    // Work small: creases are long, so 400 px sees them and stays fast.
    let (w0, h0) = texture.dimensions();
    let scale = (400.0 / w0.max(h0) as f32).min(1.0);
    let (w, h) = (
        ((w0 as f32 * scale) as u32).max(8),
        ((h0 as f32 * scale) as u32).max(8),
    );
    let small = image::imageops::resize(texture, w, h, image::imageops::FilterType::Triangle);
    let blur = image::imageops::blur(&small, 6.0);
    let hp: Vec<f32> = small
        .pixels()
        .zip(blur.pixels())
        .map(|(a, b)| f32::from(a.0[0]) - f32::from(b.0[0]))
        .collect();
    let at = |x: i64, y: i64| -> Option<f32> {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            None
        } else {
            Some(hp[(y as u32 * w + x as u32) as usize])
        }
    };
    let mut found = Vec::new();
    for vertical in [true, false] {
        let (along, across) = if vertical { (h, w) } else { (w, h) };
        let mut best: Vec<(f32, u32, f32)> = Vec::new(); // (mean, position, angle)
        for angle_step in -15..=15 {
            let angle = angle_step as f32;
            let slope = angle.to_radians().tan();
            for pos in 0..across {
                let mut sum = 0.0;
                let mut n = 0;
                for t in 0..along {
                    let offset = (t as f32 - along as f32 / 2.0) * slope;
                    let p = (pos as f32 + offset).round() as i64;
                    let v = if vertical {
                        at(p, t as i64)
                    } else {
                        at(t as i64, p)
                    };
                    if let Some(v) = v {
                        sum += v;
                        n += 1;
                    }
                }
                if n as f32 > along as f32 * 0.8 {
                    best.push((sum / n as f32, pos, angle));
                }
            }
        }
        if best.len() < 10 {
            continue;
        }
        let mean = best.iter().map(|b| b.0).sum::<f32>() / best.len() as f32;
        let sd = (best.iter().map(|b| (b.0 - mean).powi(2)).sum::<f32>() / best.len() as f32)
            .sqrt()
            .max(1e-3);
        let mut hits: Vec<&(f32, u32, f32)> = best
            .iter()
            .filter(|b| ((b.0 - mean) / sd).abs() > 6.0)
            .collect();
        hits.sort_by(|a, b| (b.0 - mean).abs().total_cmp(&(a.0 - mean).abs()));
        let mut kept: Vec<&(f32, u32, f32)> = Vec::new();
        for hit in hits {
            if kept.iter().all(|k| (k.1 as i64 - hit.1 as i64).abs() > 12) {
                kept.push(hit);
            }
        }
        for k in kept.into_iter().take(4) {
            found.push(Crease {
                orientation: if vertical { "vertical" } else { "horizontal" }.into(),
                position: (k.1 as f32 / scale).round() as u32,
                angle_degrees: k.2,
                strength: ((k.0 - mean) / sd).abs(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_is_deterministic_and_centred() {
        let a = paper(64, 64, 7, 1.0);
        let b = paper(64, 64, 7, 1.0);
        assert_eq!(a, b);
        let mean = a.pixels().map(|p| f32::from(p.0[0])).sum::<f32>() / (64.0 * 64.0);
        assert!((mean - 128.0).abs() < 1.0, "{mean}");
        assert_ne!(paper(64, 64, 8, 1.0), a);
    }

    #[test]
    fn generated_paper_has_no_creases() {
        assert!(find_creases(&paper(300, 300, 3, 1.0)).is_empty());
    }

    #[test]
    fn a_folded_sheet_is_caught() {
        let mut sheet = paper(300, 300, 3, 1.0);
        for y in 0..300u32 {
            let x = 200 - y / 12; // a slightly slanted crease, like a fold
            for dx in 0..3 {
                sheet.put_pixel(x + dx, y, Luma([40]));
            }
        }
        for x in 0..300 {
            sheet.put_pixel(x, 120, Luma([45]));
            sheet.put_pixel(x, 121, Luma([45]));
        }
        let creases = find_creases(&sheet);
        assert!(
            creases.iter().any(|c| c.orientation == "vertical"),
            "{creases:?}"
        );
        assert!(
            creases
                .iter()
                .any(|c| c.orientation == "horizontal" && (c.position as i64 - 120).abs() < 6),
            "{creases:?}"
        );
    }
}
