//! Named, explicit mask edits.
//!
//! The model's mask is the default and is never "improved" behind anyone's
//! back. Every change is one of these operations, applied in order, so an edit
//! can be shown, reviewed, undone (drop it from the list) and replayed.

use crate::error::{KataError, Result};
use crate::mask::Mask;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Where to look when segmenting. Running the model on a crop around the
/// subject is what makes it lock onto one thing in a busy street.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hint {
    /// The whole image.
    #[default]
    None,
    /// A box around the subject, in source pixels. `margin` pads it (a fraction of its size).
    Box {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        #[serde(default = "default_margin")]
        margin: f32,
    },
    /// Points on the subject. The whole image is segmented, then only the
    /// pieces that touch a point are kept.
    Points { points: Vec<[u32; 2]> },
}

fn default_margin() -> f32 {
    0.08
}

/// The region of the source the model should see for a hint: (x, y, w, h).
pub fn hint_region(hint: &Hint, width: u32, height: u32) -> Result<(u32, u32, u32, u32)> {
    match hint {
        Hint::None | Hint::Points { .. } => Ok((0, 0, width, height)),
        Hint::Box { x, y, w, h, margin } => {
            if *w == 0 || *h == 0 || *x >= width || *y >= height {
                return Err(KataError::Invalid(format!(
                    "the hint box {x},{y} {w}x{h} is outside the {width}x{height} image"
                )));
            }
            let mx = (*w as f32 * margin.clamp(0.0, 1.0)).round() as u32;
            let my = (*h as f32 * margin.clamp(0.0, 1.0)).round() as u32;
            let x0 = x.saturating_sub(mx);
            let y0 = y.saturating_sub(my);
            let x1 = (x + w + mx).min(width);
            let y1 = (y + h + my).min(height);
            Ok((x0, y0, x1 - x0, y1 - y0))
        }
    }
}

/// Put a mask computed for a crop back into a full-size, otherwise empty mask.
pub fn place(crop: &Mask, x0: u32, y0: u32, width: u32, height: u32) -> Mask {
    let mut out = Mask::new(width, height, 0);
    for y in 0..crop.height {
        for x in 0..crop.width {
            let (fx, fy) = (x0 + x, y0 + y);
            if fx < width && fy < height {
                out.set(fx, fy, crop.get(x, y));
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MaskOp {
    /// Keep the biggest solid piece; drop everything else.
    KeepLargest,
    /// Keep only the pieces that contain one of these points.
    KeepAt { points: Vec<[u32; 2]> },
    /// Drop solid pieces smaller than this many pixels (specks, slivers of chair).
    DropSpecks { min_area: u32 },
    /// Fill holes smaller than this many pixels. Large holes (the gap between an
    /// arm and a body) are background and stay open.
    FillHoles { max_area: u32 },
    /// Make everything inside the polygon transparent.
    Remove { polygon: Vec<[f32; 2]> },
    /// Make everything inside the polygon opaque.
    Add { polygon: Vec<[f32; 2]> },
    /// Grow the subject outward by whole pixels.
    Grow { px: u32 },
    /// Shrink the subject inward by whole pixels.
    Shrink { px: u32 },
    /// Soften the edge with a blur of about this many pixels.
    Feather { px: f32 },
    /// Steepen the edge: values at or below `low` become 0, at or above `high` 255.
    Harden { low: u8, high: u8 },
    /// Edge-aware refinement guided by the photo, for hair and fur.
    Matte {
        #[serde(default = "default_matte_radius")]
        radius: u32,
        #[serde(default = "default_matte_epsilon")]
        epsilon: f32,
    },
}

fn default_matte_radius() -> u32 {
    8
}
fn default_matte_epsilon() -> f32 {
    1e-4
}

/// Apply edits in order. `image` is needed only by `Matte` and must match the mask size.
pub fn apply(mask: &Mask, image: Option<&RgbaImage>, ops: &[MaskOp]) -> Result<Mask> {
    let mut current = mask.clone();
    for op in ops {
        current = apply_one(&current, image, op)?;
    }
    Ok(current)
}

fn apply_one(mask: &Mask, image: Option<&RgbaImage>, op: &MaskOp) -> Result<Mask> {
    Ok(match op {
        MaskOp::KeepLargest => {
            let (labels, sizes) = components(mask, true);
            match sizes.iter().enumerate().max_by_key(|(_, s)| **s) {
                Some((largest, _)) => keep_labels(mask, &labels, &[largest as u32 + 1]),
                None => mask.clone(),
            }
        }
        MaskOp::KeepAt { points } => {
            let (labels, _) = components(mask, true);
            let mut keep = Vec::new();
            for [x, y] in points {
                if *x < mask.width && *y < mask.height {
                    let l = labels[(*y * mask.width + *x) as usize];
                    if l > 0 {
                        keep.push(l);
                    }
                }
            }
            if keep.is_empty() {
                return Err(KataError::Invalid(
                    "none of the points is on the subject; the mask is empty there".into(),
                ));
            }
            keep_labels(mask, &labels, &keep)
        }
        MaskOp::DropSpecks { min_area } => {
            let (labels, sizes) = components(mask, true);
            let keep: Vec<u32> = sizes
                .iter()
                .enumerate()
                .filter(|(_, s)| **s >= *min_area)
                .map(|(i, _)| i as u32 + 1)
                .collect();
            keep_labels(mask, &labels, &keep)
        }
        MaskOp::FillHoles { max_area } => fill_small_holes(mask, *max_area),
        MaskOp::Remove { polygon } => paint_polygon(mask, polygon, 0)?,
        MaskOp::Add { polygon } => paint_polygon(mask, polygon, 255)?,
        MaskOp::Grow { px } => mask.dilate(*px),
        MaskOp::Shrink { px } => mask.erode(*px),
        MaskOp::Feather { px } => mask.blur((*px / 2.0).max(0.0)),
        MaskOp::Harden { low, high } => {
            if low >= high {
                return Err(KataError::Invalid(format!(
                    "harden needs low < high, got {low} and {high}"
                )));
            }
            let span = f32::from(high - low);
            Mask {
                width: mask.width,
                height: mask.height,
                data: mask
                    .data
                    .iter()
                    .map(|&v| {
                        if v <= *low {
                            0
                        } else if v >= *high {
                            255
                        } else {
                            ((f32::from(v - low) / span) * 255.0).round() as u8
                        }
                    })
                    .collect(),
            }
        }
        MaskOp::Matte { radius, epsilon } => {
            let image = image.ok_or_else(|| KataError::Invalid("matte needs the photo".into()))?;
            crate::matte::guided(mask, image, *radius, *epsilon)?
        }
    })
}

/// Label the solid (>= 128) or empty (< 128) pieces, 4-connected.
/// Returns a label per pixel (0 = not in the set) and each label's size.
pub fn components(mask: &Mask, solid: bool) -> (Vec<u32>, Vec<u32>) {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let inside = |i: usize| (mask.data[i] >= 128) == solid;
    let mut labels = vec![0u32; w * h];
    let mut sizes = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..w * h {
        if labels[start] != 0 || !inside(start) {
            continue;
        }
        let label = sizes.len() as u32 + 1;
        let mut size = 0u32;
        labels[start] = label;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            size += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if labels[j] == 0 && inside(j) {
                    labels[j] = label;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        sizes.push(size);
    }
    (labels, sizes)
}

fn keep_labels(mask: &Mask, labels: &[u32], keep: &[u32]) -> Mask {
    // Soft edge pixels below the threshold belong to whichever kept piece they
    // touch, so the anti-aliased rim survives instead of becoming a hard step.
    let solid_kept: Vec<bool> = labels.iter().map(|l| *l > 0 && keep.contains(l)).collect();
    let near = Mask {
        width: mask.width,
        height: mask.height,
        data: solid_kept
            .iter()
            .map(|k| if *k { 255 } else { 0 })
            .collect(),
    }
    .dilate(2);
    Mask {
        width: mask.width,
        height: mask.height,
        data: mask
            .data
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                if solid_kept[i] || (v < 128 && near.data[i] > 0) {
                    v
                } else {
                    0
                }
            })
            .collect(),
    }
}

fn fill_small_holes(mask: &Mask, max_area: u32) -> Mask {
    let (labels, sizes) = components(mask, false);
    let (w, h) = (mask.width as usize, mask.height as usize);
    // A hole is an empty piece that does not reach the border.
    let mut touches_border = vec![false; sizes.len() + 1];
    for x in 0..w {
        touches_border[labels[x] as usize] = true;
        touches_border[labels[(h - 1) * w + x] as usize] = true;
    }
    for y in 0..h {
        touches_border[labels[y * w] as usize] = true;
        touches_border[labels[y * w + w - 1] as usize] = true;
    }
    let mut out = mask.clone();
    for (i, label) in labels.iter().enumerate() {
        if *label > 0 && !touches_border[*label as usize] && sizes[*label as usize - 1] <= max_area
        {
            out.data[i] = 255;
        }
    }
    out
}

/// Even-odd scanline fill, sampling each pixel at its centre.
fn paint_polygon(mask: &Mask, polygon: &[[f32; 2]], value: u8) -> Result<Mask> {
    if polygon.len() < 3 {
        return Err(KataError::Invalid(
            "a polygon needs at least three points".into(),
        ));
    }
    let mut out = mask.clone();
    for y in 0..mask.height {
        let sy = y as f32 + 0.5;
        let mut crossings: Vec<f32> = Vec::new();
        for i in 0..polygon.len() {
            let [x1, y1] = polygon[i];
            let [x2, y2] = polygon[(i + 1) % polygon.len()];
            if (y1 <= sy && y2 > sy) || (y2 <= sy && y1 > sy) {
                crossings.push(x1 + (sy - y1) / (y2 - y1) * (x2 - x1));
            }
        }
        crossings.sort_by(f32::total_cmp);
        for pair in crossings.chunks(2) {
            if let [a, b] = pair {
                let start = (a - 0.5).ceil().max(0.0) as u32;
                let end = ((b - 0.5).floor() as i64).min(mask.width as i64 - 1);
                if end < 0 {
                    continue;
                }
                for x in start..=end as u32 {
                    out.set(x, y, value);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_with_speck() -> Mask {
        let mut m = Mask::new(20, 20, 0);
        for y in 4..14 {
            for x in 4..14 {
                m.set(x, y, 255);
            }
        }
        m.set(18, 18, 255);
        m
    }

    #[test]
    fn keep_largest_drops_the_speck() {
        let out = apply(&square_with_speck(), None, &[MaskOp::KeepLargest]).unwrap();
        assert_eq!(out.get(18, 18), 0);
        assert_eq!(out.get(8, 8), 255);
    }

    #[test]
    fn keep_at_keeps_only_the_touched_piece() {
        let out = apply(
            &square_with_speck(),
            None,
            &[MaskOp::KeepAt {
                points: vec![[18, 18]],
            }],
        )
        .unwrap();
        assert_eq!(out.get(18, 18), 255);
        assert_eq!(out.get(8, 8), 0);
        let err = apply(
            &square_with_speck(),
            None,
            &[MaskOp::KeepAt {
                points: vec![[1, 1]],
            }],
        );
        assert!(err.is_err());
    }

    #[test]
    fn small_holes_fill_and_big_ones_stay_open() {
        let mut m = Mask::new(30, 30, 0);
        for y in 2..28 {
            for x in 2..28 {
                m.set(x, y, 255);
            }
        }
        m.set(5, 5, 0); // a pinhole
        for y in 10..20 {
            for x in 10..20 {
                m.set(x, y, 0); // a real gap
            }
        }
        let out = apply(&m, None, &[MaskOp::FillHoles { max_area: 10 }]).unwrap();
        assert_eq!(out.get(5, 5), 255);
        assert_eq!(out.get(15, 15), 0);
    }

    #[test]
    fn polygon_remove_and_add() {
        let m = Mask::new(10, 10, 255);
        let tri = vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let out = apply(
            &m,
            None,
            &[MaskOp::Remove {
                polygon: tri.clone(),
            }],
        )
        .unwrap();
        assert_eq!(out.get(1, 1), 0);
        assert_eq!(out.get(9, 9), 255);
        let back = apply(&out, None, &[MaskOp::Add { polygon: tri }]).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn harden_steepens_and_rejects_a_backwards_range() {
        let m = Mask {
            width: 3,
            height: 1,
            data: vec![50, 128, 210],
        };
        let out = apply(&m, None, &[MaskOp::Harden { low: 64, high: 192 }]).unwrap();
        assert_eq!(out.data, vec![0, 128, 255]);
        assert!(apply(&m, None, &[MaskOp::Harden { low: 9, high: 9 }]).is_err());
    }

    #[test]
    fn a_box_hint_is_padded_and_clamped() {
        let hint = Hint::Box {
            x: 10,
            y: 10,
            w: 100,
            h: 50,
            margin: 0.1,
        };
        assert_eq!(hint_region(&hint, 115, 200).unwrap(), (0, 5, 115, 60));
        assert!(hint_region(
            &Hint::Box {
                x: 500,
                y: 0,
                w: 1,
                h: 1,
                margin: 0.0
            },
            100,
            100
        )
        .is_err());
    }

    #[test]
    fn ops_round_trip_through_json() {
        let ops = vec![
            MaskOp::KeepLargest,
            MaskOp::FillHoles { max_area: 200 },
            MaskOp::Feather { px: 1.5 },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        assert!(json.contains("\"op\":\"fill_holes\""), "{json}");
        let back: Vec<MaskOp> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ops);
    }
}
