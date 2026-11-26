//! The checks that catch a poster's mistakes before a person does.
//!
//! Every one of these exists because the mistake happened: an elbow printed in
//! the ground colour vanished, a chair survived as a sliver beside a man's arm,
//! faces went black, a logo sank into a coral ground, a paper scan put a fold
//! through every poster. Each finding carries a box so a reviewer (or an agent)
//! can crop straight to it.

use crate::color::Rgba;
use crate::lab::{contrast_ratio, Lab};
use crate::mask::Mask;
use crate::ops::components;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Will be visible to everyone. Fix before publishing.
    Error,
    /// Probably visible. Look at the crop.
    Warning,
    /// Worth knowing.
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub check: String,
    pub severity: Severity,
    pub message: String,
    /// x, y, w, h in the inspected image's pixels.
    pub bbox: Option<[u32; 4]>,
}

impl Finding {
    fn new(check: &str, severity: Severity, message: String, bbox: Option<[u32; 4]>) -> Finding {
        Finding {
            check: check.into(),
            severity,
            message,
            bbox,
        }
    }
}

fn bbox_of(labels: &[u32], label: u32, width: u32) -> [u32; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, l) in labels.iter().enumerate() {
        if *l == label {
            let (x, y) = (i as u32 % width, i as u32 / width);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    [x0, y0, x1 - x0 + 1, y1 - y0 + 1]
}

/// Parts of a printed subject that are the ground's colour: they will vanish.
pub fn ink_against_ground(layer: &RgbaImage, ground: Rgba) -> Vec<Finding> {
    let g = Lab::from_rgba(ground);
    let (w, h) = layer.dimensions();
    let mut same = Mask::new(w, h, 0);
    let mut opaque = 0u64;
    for (x, y, p) in layer.enumerate_pixels() {
        if p.0[3] > 200 {
            opaque += 1;
            if Lab::from_rgb(p.0[0], p.0[1], p.0[2]).delta_e(&g) < 10.0 {
                same.set(x, y, 255);
            }
        }
    }
    if opaque == 0 {
        return Vec::new();
    }
    let (labels, sizes) = components(&same, true);
    let min = (opaque as f32 * 0.004).max(64.0) as u32;
    let mut out: Vec<Finding> = sizes
        .iter()
        .enumerate()
        .filter(|(_, s)| **s >= min)
        .map(|(i, s)| {
            let share = *s as f32 / opaque as f32 * 100.0;
            Finding::new(
                "ink_against_ground",
                if share > 2.0 { Severity::Error } else { Severity::Warning },
                format!(
                    "{share:.1}% of the subject is printed in the ground colour {} here, so it will disappear into the background. Use a different ink for this tone.",
                    ground.to_hex()
                ),
                Some(bbox_of(&labels, i as u32 + 1, w)),
            )
        })
        .collect();
    out.sort_by(|a, b| a.message.cmp(&b.message));
    out
}

/// Leftovers in a cutout mask: loose specks and thin slivers (a chair leg, a railing).
pub fn mask_leftovers(mask: &Mask) -> Vec<Finding> {
    let mut out = Vec::new();
    let (labels, sizes) = components(mask, true);
    let total: u32 = sizes.iter().sum();
    if total == 0 {
        return vec![Finding::new(
            "mask_empty",
            Severity::Error,
            "the mask is empty: nothing was cut out".into(),
            None,
        )];
    }
    let largest = *sizes.iter().max().unwrap_or(&0);
    for (i, s) in sizes.iter().enumerate() {
        if *s < largest && *s > 8 {
            out.push(Finding::new(
                "mask_island",
                if *s as f32 > total as f32 * 0.01 { Severity::Warning } else { Severity::Info },
                format!("a separate piece of {s} px sits apart from the subject. Keep it only if it belongs."),
                Some(bbox_of(&labels, i as u32 + 1, mask.width)),
            ));
        }
    }
    // Slivers: what an opening (erode then dilate) removes, grouped.
    let opened = mask.thresholded(128).erode(3).dilate(3);
    let thin = Mask {
        width: mask.width,
        height: mask.height,
        data: mask
            .data
            .iter()
            .zip(&opened.data)
            .map(|(m, o)| if *m >= 128 && *o < 128 { 255 } else { 0 })
            .collect(),
    };
    let (tl, ts) = components(&thin, true);
    let min = (total as f32 * 0.0015).max(40.0) as u32;
    for (i, s) in ts.iter().enumerate() {
        if *s >= min {
            let bbox = bbox_of(&tl, i as u32 + 1, mask.width);
            // Long and thin, not just the rim of a finger.
            let (bw, bh) = (bbox[2] as f32, bbox[3] as f32);
            if bw.max(bh) / bw.min(bh).max(1.0) > 3.0 {
                out.push(Finding::new(
                    "mask_sliver",
                    Severity::Warning,
                    format!("a thin strip of {s} px is attached to the subject: often furniture or a railing the model kept. Check the crop."),
                    Some(bbox),
                ));
            }
        }
    }
    out
}

/// Is `fg` readable on `bg`? Uses WCAG ratios: 3 for large display type and logos, 4.5 for text.
pub fn legibility(
    what: &str,
    fg: Rgba,
    bg: Rgba,
    large: bool,
    bbox: Option<[u32; 4]>,
) -> Option<Finding> {
    let ratio = contrast_ratio(fg, bg);
    let need = if large { 3.0 } else { 4.5 };
    if ratio >= need {
        return None;
    }
    Some(Finding::new(
        "legibility",
        if ratio < need * 0.67 { Severity::Error } else { Severity::Warning },
        format!(
            "{what} in {} on {} has contrast {ratio:.2}:1; it needs {need}:1. Use a one-colour version or a darker ink.",
            fg.to_hex(),
            bg.to_hex()
        ),
        bbox,
    ))
}

/// Regions (faces) that printed too dark to read.
pub fn dark_regions(
    image: &RgbaImage,
    regions: &[(String, [u32; 4])],
    min_luma: u8,
) -> Vec<Finding> {
    let (w, h) = image.dimensions();
    regions
        .iter()
        .filter_map(|(name, [x, y, bw, bh])| {
            let (x1, y1) = ((x + bw).min(w), (y + bh).min(h));
            let mut sum = 0u64;
            let mut n = 0u64;
            let mut distinct = std::collections::HashSet::new();
            for yy in *y..y1 {
                for xx in *x..x1 {
                    let p = Rgba::from(*image.get_pixel(xx, yy));
                    if p.a > 128 {
                        sum += u64::from(p.luma());
                        n += 1;
                        distinct.insert(p.to_hex());
                    }
                }
            }
            if n == 0 {
                return None;
            }
            let mean = (sum / n) as u8;
            (mean < min_luma || distinct.len() < 2).then(|| {
                Finding::new(
                    "dark_region",
                    Severity::Error,
                    format!(
                        "{name} averages luma {mean} with {} ink(s): features will not read. Lift shadows (prep gamma 0.7, clahe 40) before printing.",
                        distinct.len()
                    ),
                    Some([*x, *y, x1 - x, y1 - y]),
                )
            })
        })
        .collect()
}

/// Two boxes that overlap by more than `tolerance` of the smaller one.
pub fn overlaps(boxes: &[(String, [u32; 4])], tolerance: f32) -> Vec<Finding> {
    let mut out = Vec::new();
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            let (a, b) = (&boxes[i].1, &boxes[j].1);
            let ix = (a[0] + a[2])
                .min(b[0] + b[2])
                .saturating_sub(a[0].max(b[0]));
            let iy = (a[1] + a[3])
                .min(b[1] + b[3])
                .saturating_sub(a[1].max(b[1]));
            let inter = (ix * iy) as f32;
            let smaller = ((a[2] * a[3]).min(b[2] * b[3])).max(1) as f32;
            if inter / smaller > tolerance {
                out.push(Finding::new(
                    "overlap",
                    Severity::Warning,
                    format!(
                        "`{}` and `{}` overlap by {:.0}% of the smaller one.",
                        boxes[i].0,
                        boxes[j].0,
                        inter / smaller * 100.0
                    ),
                    Some([a[0].max(b[0]), a[1].max(b[1]), ix, iy]),
                ));
            }
        }
    }
    out
}

/// Elements that sit outside a target's safe area (where app chrome covers the post).
pub fn safe_area(target: &crate::targets::Target, boxes: &[(String, [u32; 4])]) -> Vec<Finding> {
    let [top, right, bottom, left] = target.safe;
    boxes
        .iter()
        .filter(|(_, [x, y, w, h])| *x < left || *y < top || x + w > target.width - right || y + h > target.height - bottom)
        .map(|(name, b)| {
            Finding::new(
                "safe_area",
                Severity::Warning,
                format!("`{name}` reaches outside the {} safe area, where the app's own buttons may cover it.", target.id),
                Some(*b),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elbow_in_the_ground_colour_is_reported() {
        let ground = Rgba::rgb(0xFF, 0x7A, 0x2E);
        let mut layer = RgbaImage::new(100, 100);
        for (x, y, p) in layer.enumerate_pixels_mut() {
            if (20..80).contains(&x) && (10..90).contains(&y) {
                *p = if x > 60 && y > 50 {
                    image::Rgba([0xFF, 0x7A, 0x2E, 255])
                } else {
                    image::Rgba([0x7A, 0x18, 0x12, 255])
                };
            }
        }
        let found = ink_against_ground(&layer, ground);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].bbox, Some([61, 51, 19, 39]));
    }

    #[test]
    fn a_chair_leg_sliver_and_an_island_are_reported() {
        let mut m = Mask::new(120, 120, 0);
        for y in 10..100 {
            for x in 30..80 {
                m.set(x, y, 255);
            }
        }
        // A chair leg: a thin strip a little apart from the body, joined at one point.
        for y in 40..110 {
            m.set(86, y, 255);
            m.set(87, y, 255);
        }
        for x in 80..86 {
            for y in 90..93 {
                m.set(x, y, 255);
            }
        }
        for y in 5..9 {
            for x in 100..104 {
                m.set(x, y, 255);
            }
        }
        let found = mask_leftovers(&m);
        assert!(found.iter().any(|f| f.check == "mask_sliver"), "{found:?}");
        assert!(found.iter().any(|f| f.check == "mask_island"), "{found:?}");
    }

    #[test]
    fn a_full_colour_logo_on_coral_is_flagged() {
        let found = legibility(
            "logo",
            Rgba::rgb(0xF9, 0xEC, 0xDD),
            Rgba::rgb(0xF0, 0x5A, 0x47),
            true,
            None,
        );
        assert!(found.is_some());
        assert!(legibility(
            "logo",
            Rgba::rgb(0x2A, 0x13, 0x08),
            Rgba::rgb(0xF0, 0x5A, 0x47),
            true,
            None
        )
        .is_none());
    }

    #[test]
    fn a_black_face_is_reported() {
        let img = RgbaImage::from_pixel(50, 50, image::Rgba([20, 10, 5, 255]));
        assert_eq!(
            dark_regions(&img, &[("face".into(), [10, 10, 20, 20])], 60).len(),
            1
        );
    }

    #[test]
    fn overlapping_text_and_art_are_reported() {
        let found = overlaps(
            &[
                ("headline".into(), [0, 0, 100, 50]),
                ("tray".into(), [80, 20, 100, 100]),
            ],
            0.05,
        );
        assert_eq!(found.len(), 1);
    }
}
