//! Printing a photo (or the subject of a photo) in a few flat inks.
//!
//! Three modes, because one does not fit every picture:
//!
//! * **tonal**: inks are ranked dark to light and each pixel takes the ink for
//!   its lightness. Portraits and single-colour scenes.
//! * **spot**: each patch takes the nearest ink by *colour* (Lab, hue weighted
//!   over lightness). A market full of umbrellas stays a market full of colours.
//! * **line**: thin dark strokes (pencil, pen, wire) are found on their own and
//!   laid over the print in one ink, like a separate plate. Combine with either.
//!
//! Tone prep runs first: local contrast and a gamma lift so faces keep their
//! features instead of collapsing into the darkest ink.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use crate::filters;
use crate::lab::Lab;
use crate::mask::Mask;
use crate::posterize::{self, PosterizeOptions};
use image::{GrayImage, Luma, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrintMode {
    #[default]
    Tonal,
    Spot,
}

/// Before printing: open up shadows so faces stay readable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prep {
    /// Local contrast (CLAHE) 0..=100. 0 is off.
    #[serde(default)]
    pub clahe: u8,
    /// Below 1.0 lifts shadows (0.72 rescued the faces on a backlit group shot).
    #[serde(default = "one")]
    pub gamma: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for Prep {
    fn default() -> Self {
        Prep {
            clahe: 0,
            gamma: 1.0,
        }
    }
}

/// Inside a region, an ink that should not be there is swapped for another.
/// The reason this exists: paper-coloured highlights on a face read as face paint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionRule {
    pub polygon: Vec<[f32; 2]>,
    pub forbid: Vec<String>,
    pub replace_with: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinePlate {
    pub ink: String,
    /// 0..=100: how strong a stroke must be to print. Higher prints fewer lines.
    #[serde(default = "default_line_threshold")]
    pub threshold: u8,
    /// Stroke width the detector looks for, in pixels.
    #[serde(default = "default_line_width")]
    pub width: u32,
}

fn default_line_threshold() -> u8 {
    40
}
fn default_line_width() -> u32 {
    7
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintSpec {
    #[serde(default)]
    pub mode: PrintMode,
    /// Hex inks. Tonal sorts them dark to light; spot uses them as given.
    pub inks: Vec<String>,
    #[serde(default)]
    pub prep: Prep,
    /// Edge-preserving smoothing 0..=100.
    #[serde(default = "default_smooth")]
    pub smooth: u8,
    /// Spot mode: how much hue outweighs lightness when choosing an ink.
    #[serde(default = "default_hue_weight")]
    pub hue_weight: f32,
    /// Despeckle radius on the ink map: 0, 1 or 2.
    #[serde(default = "default_despeckle")]
    pub despeckle: u32,
    #[serde(default)]
    pub regions: Vec<RegionRule>,
    #[serde(default)]
    pub line: Option<LinePlate>,
}

fn default_smooth() -> u8 {
    50
}
fn default_hue_weight() -> f32 {
    2.5
}
fn default_despeckle() -> u32 {
    1
}

impl PrintSpec {
    pub fn new(mode: PrintMode, inks: &[&str]) -> PrintSpec {
        PrintSpec {
            mode,
            inks: inks.iter().map(|s| s.to_string()).collect(),
            prep: Prep::default(),
            smooth: default_smooth(),
            hue_weight: default_hue_weight(),
            despeckle: default_despeckle(),
            regions: Vec::new(),
            line: None,
        }
    }

    fn colours(&self) -> Result<Vec<Rgba>> {
        if self.inks.len() < 2 {
            return Err(KataError::Invalid("a print needs at least two inks".into()));
        }
        let max = match self.mode {
            PrintMode::Tonal => 5,
            PrintMode::Spot => 16,
        };
        if self.inks.len() > max {
            return Err(KataError::Invalid(format!(
                "{} inks were given; {:?} printing takes at most {max}",
                self.inks.len(),
                self.mode
            )));
        }
        self.inks.iter().map(|h| Rgba::from_hex(h)).collect()
    }
}

/// Lift shadows and local contrast before printing.
pub fn prep(image: &RgbaImage, prep: &Prep) -> RgbaImage {
    let mut out = image.clone();
    if (prep.gamma - 1.0).abs() > 1e-3 {
        let lut: Vec<u8> = (0..256)
            .map(|v| (255.0 * (v as f32 / 255.0).powf(prep.gamma.clamp(0.2, 3.0))).round() as u8)
            .collect();
        for p in out.pixels_mut() {
            for c in 0..3 {
                p.0[c] = lut[p.0[c] as usize];
            }
        }
    }
    if prep.clahe > 0 {
        let luma = filters::to_luma(&out);
        let lifted = filters::clahe(&luma, 8, 1.0 + f32::from(prep.clahe.min(100)) * 0.04);
        for (p, (before, after)) in out.pixels_mut().zip(luma.pixels().zip(lifted.pixels())) {
            let (b, a) = (f32::from(before.0[0]).max(1.0), f32::from(after.0[0]));
            let k = a / b;
            for c in 0..3 {
                p.0[c] = (f32::from(p.0[c]) * k).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Print `image`. With a mask, only the subject is printed and the rest is
/// transparent; cut points then come from the subject alone.
pub fn print(image: &RgbaImage, mask: Option<&Mask>, spec: &PrintSpec) -> Result<RgbaImage> {
    let inks = spec.colours()?;
    if let Some(m) = mask {
        if m.dimensions() != image.dimensions() {
            return Err(KataError::Invalid(
                "the mask and the image are different sizes".into(),
            ));
        }
    }
    let mut source = prep(image, &spec.prep);
    if let Some(m) = mask {
        for (p, a) in source.pixels_mut().zip(&m.data) {
            p.0[3] = *a;
        }
    }

    let mut out = match spec.mode {
        PrintMode::Tonal => {
            let mut sorted = inks.clone();
            sorted.sort_by(|a, b| Lab::from_rgba(*a).l.total_cmp(&Lab::from_rgba(*b).l));
            let options = PosterizeOptions {
                palette: sorted,
                smooth: spec.smooth,
                clahe: 0,
                highlight_clip: 0.0,
                median_radius: spec.despeckle.min(2),
                working_side: 1600,
                registration: 0,
            };
            posterize::posterize(&source, &options)?.image
        }
        PrintMode::Spot => spot(&source, &inks, spec)?,
    };

    if let Some(line) = &spec.line {
        let ink = Rgba::from_hex(&line.ink)?;
        let strokes = line_plate(image, line.width, line.threshold);
        for (p, s) in out.pixels_mut().zip(strokes.pixels()) {
            let t = f32::from(s.0[0]) / 255.0;
            if t > 0.0 && p.0[3] > 0 {
                for c in 0..3 {
                    p.0[c] = (f32::from(p.0[c]) * (1.0 - t) + f32::from(ink.to_array()[c]) * t)
                        .round() as u8;
                }
            }
        }
    }

    for rule in &spec.regions {
        apply_region(&mut out, rule)?;
    }
    Ok(out)
}

fn spot(source: &RgbaImage, inks: &[Rgba], spec: &PrintSpec) -> Result<RgbaImage> {
    let smooth = spec.smooth.min(100);
    let smoothed = if smooth == 0 {
        source.clone()
    } else {
        let radius = 1 + (u32::from(smooth) * 5) / 100;
        filters::bilateral(
            source,
            radius,
            radius as f32 * 0.8,
            10.0 + f32::from(smooth) * 0.4,
        )
    };
    let labs: Vec<Lab> = inks.iter().map(|c| Lab::from_rgba(*c)).collect();
    let (w, h) = source.dimensions();
    let mut index = GrayImage::new(w, h);
    // Cache by colour: photos repeat colours a lot after smoothing.
    let mut cache: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    for (p, i) in smoothed.pixels().zip(index.pixels_mut()) {
        let key = [p.0[0], p.0[1], p.0[2]];
        let best = *cache.entry(key).or_insert_with(|| {
            let lab = Lab::from_rgb(key[0], key[1], key[2]);
            labs.iter()
                .enumerate()
                .min_by(|a, b| {
                    lab.weighted_distance(a.1, 1.0, spec.hue_weight)
                        .total_cmp(&lab.weighted_distance(b.1, 1.0, spec.hue_weight))
                })
                .map(|(k, _)| k as u8)
                .unwrap_or(0)
        });
        *i = Luma([best]);
    }
    let index = majority(&index, spec.despeckle.min(2), inks.len());
    let mut out = RgbaImage::new(w, h);
    for ((o, i), s) in out.pixels_mut().zip(index.pixels()).zip(source.pixels()) {
        let ink = inks[i.0[0] as usize];
        *o = image::Rgba([ink.r, ink.g, ink.b, s.0[3]]);
    }
    Ok(out)
}

/// Each pixel takes the most common ink in its window: specks disappear,
/// edges between large regions stay where they were.
fn majority(index: &GrayImage, radius: u32, inks: usize) -> GrayImage {
    if radius == 0 {
        return index.clone();
    }
    let (w, h) = index.dimensions();
    let r = radius as i64;
    let mut out = GrayImage::new(w, h);
    let mut counts = vec![0u16; inks.max(1)];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            counts.iter_mut().for_each(|c| *c = 0);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (sx, sy) = (
                        (x + dx).clamp(0, w as i64 - 1),
                        (y + dy).clamp(0, h as i64 - 1),
                    );
                    let k = index.get_pixel(sx as u32, sy as u32).0[0] as usize;
                    if k < counts.len() {
                        counts[k] += 1;
                    }
                }
            }
            let centre = index.get_pixel(x as u32, y as u32).0[0];
            let (best, n) = counts.iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
            // Keep the centre on a tie, so edges do not drift.
            let pick = if counts[centre as usize] == *n {
                centre
            } else {
                best as u8
            };
            out.put_pixel(x as u32, y as u32, Luma([pick]));
        }
    }
    out
}

/// Thin dark strokes: the morphological black-hat (closing minus the image).
/// Returns stroke strength 0..=255 per pixel.
pub fn line_plate(image: &RgbaImage, width: u32, threshold: u8) -> GrayImage {
    let luma = filters::to_luma(image);
    let r = (width.max(3) / 2) as usize;
    let closed = min_filter(&max_filter(&luma, r), r);
    let (w, h) = luma.dimensions();
    let floor = f32::from(threshold.min(100)) * 0.6;
    let mut out = GrayImage::new(w, h);
    for ((o, c), l) in out.pixels_mut().zip(closed.pixels()).zip(luma.pixels()) {
        let hat = f32::from(c.0[0].saturating_sub(l.0[0]));
        let t = ((hat - floor) / 24.0).clamp(0.0, 1.0);
        *o = Luma([(t * 255.0) as u8]);
    }
    out
}

fn filter_1d(src: &GrayImage, r: usize, horizontal: bool, pick_max: bool) -> GrayImage {
    let (w, h) = src.dimensions();
    let mut out = GrayImage::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let mut v = if pick_max { 0u8 } else { 255u8 };
            let (lo, hi, fixed) = if horizontal {
                (x.saturating_sub(r), (x + r).min(w as usize - 1), y)
            } else {
                (y.saturating_sub(r), (y + r).min(h as usize - 1), x)
            };
            for k in lo..=hi {
                let s = if horizontal {
                    src.get_pixel(k as u32, fixed as u32).0[0]
                } else {
                    src.get_pixel(fixed as u32, k as u32).0[0]
                };
                v = if pick_max { v.max(s) } else { v.min(s) };
            }
            out.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }
    out
}

fn max_filter(src: &GrayImage, r: usize) -> GrayImage {
    filter_1d(&filter_1d(src, r, true, true), r, false, true)
}

fn min_filter(src: &GrayImage, r: usize) -> GrayImage {
    filter_1d(&filter_1d(src, r, true, false), r, false, false)
}

fn apply_region(out: &mut RgbaImage, rule: &RegionRule) -> Result<()> {
    let replace = Rgba::from_hex(&rule.replace_with)?;
    let forbid = rule
        .forbid
        .iter()
        .map(|h| Rgba::from_hex(h))
        .collect::<Result<Vec<_>>>()?;
    let (w, h) = out.dimensions();
    let region = crate::ops::apply(
        &Mask::new(w, h, 0),
        None,
        &[crate::ops::MaskOp::Add {
            polygon: rule.polygon.clone(),
        }],
    )?;
    for (p, inside) in out.pixels_mut().zip(&region.data) {
        if *inside > 0 && p.0[3] > 0 {
            let here = Rgba::rgb(p.0[0], p.0[1], p.0[2]);
            if forbid
                .iter()
                .any(|f| Lab::from_rgba(*f).delta_e(&Lab::from_rgba(here)) < 3.0)
            {
                p.0[0] = replace.r;
                p.0[1] = replace.g;
                p.0[2] = replace.b;
            }
        }
    }
    Ok(())
}

/// A two-layer print: the whole scene quietly in `background`, the subject
/// at full strength in `subject` on top. The street, and the matatu in it.
pub fn print_scene(
    image: &RgbaImage,
    mask: &Mask,
    background: &PrintSpec,
    subject: &PrintSpec,
) -> Result<RgbaImage> {
    let mut base = print(image, None, background)?;
    let top = print(image, Some(mask), subject)?;
    for (b, t) in base.pixels_mut().zip(top.pixels()) {
        let a = f32::from(t.0[3]) / 255.0;
        for c in 0..3 {
            b.0[c] = (f32::from(b.0[c]) * (1.0 - a) + f32::from(t.0[c]) * a).round() as u8;
        }
        b.0[3] = 255;
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripes() -> RgbaImage {
        // Orange, a darker orange, and blue: spot should keep oranges orange.
        let mut img = RgbaImage::new(60, 20);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = match x / 20 {
                0 => image::Rgba([250, 120, 30, 255]),
                1 => image::Rgba([150, 70, 15, 255]),
                _ => image::Rgba([40, 60, 200, 255]),
            };
        }
        img
    }

    #[test]
    fn spot_mode_keeps_hue_over_lightness() {
        let spec = PrintSpec {
            smooth: 0,
            ..PrintSpec::new(PrintMode::Spot, &["#F85647", "#2A1630", "#3050D0"])
        };
        let out = print(&stripes(), None, &spec).unwrap();
        // The shaded orange is still closer to coral than to the dark plum.
        assert_eq!(out.get_pixel(30, 10).0[..3], [0xF8, 0x56, 0x47]);
        assert_eq!(out.get_pixel(50, 10).0[..3], [0x30, 0x50, 0xD0]);
    }

    #[test]
    fn tonal_output_uses_only_the_given_inks() {
        let spec = PrintSpec::new(PrintMode::Tonal, &["#F9ECDD", "#2A1308", "#F85647"]);
        let out = print(&stripes(), None, &spec).unwrap();
        let allowed = [[0xF9, 0xEC, 0xDD], [0x2A, 0x13, 0x08], [0xF8, 0x56, 0x47]];
        assert!(out
            .pixels()
            .all(|p| allowed.contains(&[p.0[0], p.0[1], p.0[2]])));
    }

    #[test]
    fn a_mask_makes_the_rest_transparent() {
        let mut mask = Mask::new(60, 20, 0);
        for y in 0..20 {
            for x in 0..30 {
                mask.set(x, y, 255);
            }
        }
        let spec = PrintSpec::new(PrintMode::Spot, &["#F85647", "#3050D0"]);
        let out = print(&stripes(), Some(&mask), &spec).unwrap();
        assert_eq!(out.get_pixel(5, 5).0[3], 255);
        assert_eq!(out.get_pixel(55, 5).0[3], 0);
    }

    #[test]
    fn a_region_rule_swaps_a_forbidden_ink() {
        let mut spec = PrintSpec::new(PrintMode::Spot, &["#F85647", "#3050D0"]);
        spec.smooth = 0;
        spec.regions.push(RegionRule {
            polygon: vec![[40.0, 0.0], [60.0, 0.0], [60.0, 20.0], [40.0, 20.0]],
            forbid: vec!["#3050D0".into()],
            replace_with: "#F85647".into(),
        });
        let out = print(&stripes(), None, &spec).unwrap();
        assert_eq!(out.get_pixel(50, 10).0[..3], [0xF8, 0x56, 0x47]);
    }

    #[test]
    fn the_line_plate_finds_a_thin_dark_stroke_and_ignores_flat_areas() {
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([230, 230, 230, 255]));
        for y in 0..40 {
            img.put_pixel(20, y, image::Rgba([30, 30, 30, 255]));
        }
        let plate = line_plate(&img, 7, 30);
        assert!(plate.get_pixel(20, 20).0[0] > 200);
        assert_eq!(plate.get_pixel(5, 20).0[0], 0);
    }

    #[test]
    fn gamma_below_one_lifts_shadows() {
        let img = RgbaImage::from_pixel(4, 4, image::Rgba([40, 40, 40, 255]));
        let lifted = prep(
            &img,
            &Prep {
                clahe: 0,
                gamma: 0.7,
            },
        );
        assert!(lifted.get_pixel(0, 0).0[0] > 60);
    }

    #[test]
    fn too_few_or_too_many_inks_are_refused() {
        assert!(print(
            &stripes(),
            None,
            &PrintSpec::new(PrintMode::Tonal, &["#000000"])
        )
        .is_err());
        let six = [
            "#000000", "#111111", "#222222", "#333333", "#444444", "#555555",
        ];
        assert!(print(&stripes(), None, &PrintSpec::new(PrintMode::Tonal, &six)).is_err());
    }
}
