//! A brand: inks with jobs, fonts, logos and voice, in one JSON file.
//!
//! Inks carry roles because "which colour goes where" is the decision that
//! breaks posters: a subject printed in the ground colour disappears into it.
//! Keeping the brand out of the code is what lets one Kata serve many projects.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use crate::lab::Lab;
use serde::{Deserialize, Serialize};

/// What an ink is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The page itself.
    Ground,
    /// Body text and the darkest shadow ink.
    Ink,
    /// The paper colour: highlights, outlines, light text on dark grounds.
    Paper,
    /// A supporting colour for headlines, stickers and print layers.
    Accent,
    /// The one thing to press. Spent sparingly.
    Action,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrandInk {
    pub name: String,
    pub hex: String,
    pub role: Role,
}

impl BrandInk {
    pub fn colour(&self) -> Result<Rgba> {
        Rgba::from_hex(&self.hex)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrandFonts {
    /// Font files by family name, e.g. {"Anton": "fonts/Anton-Regular.ttf"}.
    #[serde(default)]
    pub files: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub mono: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrandLogos {
    /// The full-colour logo, with real transparency.
    #[serde(default)]
    pub full: Option<String>,
    /// Use a one-colour version of the logo on grounds lighter than this luma (0..255).
    #[serde(default = "default_mono_above")]
    pub mono_above_luma: u8,
    /// Smallest width the logo may be drawn at, as a fraction of the canvas width.
    #[serde(default = "default_min_width")]
    pub min_width: f32,
    /// Largest, likewise. Logos drawn bigger than this read as shouting.
    #[serde(default = "default_max_width")]
    pub max_width: f32,
}

impl Default for BrandLogos {
    fn default() -> Self {
        BrandLogos {
            full: None,
            mono_above_luma: default_mono_above(),
            min_width: default_min_width(),
            max_width: default_max_width(),
        }
    }
}

fn default_mono_above() -> u8 {
    140
}
fn default_min_width() -> f32 {
    0.10
}
fn default_max_width() -> f32 {
    0.22
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brand {
    pub name: String,
    pub inks: Vec<BrandInk>,
    #[serde(default)]
    pub fonts: BrandFonts,
    #[serde(default)]
    pub logos: BrandLogos,
    /// Free text for the agent writing copy: tone, words to use and avoid.
    #[serde(default)]
    pub voice: Vec<String>,
    #[serde(default)]
    pub website: Option<String>,
}

impl Brand {
    pub fn from_json(text: &str) -> Result<Brand> {
        let brand: Brand = serde_json::from_str(text)
            .map_err(|e| KataError::Invalid(format!("brand file: {e}")))?;
        brand.validate()?;
        Ok(brand)
    }

    pub fn validate(&self) -> Result<()> {
        if self.inks.is_empty() {
            return Err(KataError::Invalid("a brand needs at least one ink".into()));
        }
        for ink in &self.inks {
            ink.colour()
                .map_err(|e| KataError::Invalid(format!("ink `{}`: {e}", ink.name)))?;
        }
        Ok(())
    }

    pub fn with_role(&self, role: Role) -> Vec<&BrandInk> {
        self.inks.iter().filter(|i| i.role == role).collect()
    }

    pub fn ink(&self, name: &str) -> Option<&BrandInk> {
        self.inks.iter().find(|i| i.name.eq_ignore_ascii_case(name))
    }

    /// Every ink as a colour, dark to light: the order the printers expect.
    pub fn inks_dark_to_light(&self) -> Result<Vec<Rgba>> {
        let mut inks = self
            .inks
            .iter()
            .map(|i| i.colour())
            .collect::<Result<Vec<_>>>()?;
        inks.sort_by(|a, b| Lab::from_rgba(*a).l.total_cmp(&Lab::from_rgba(*b).l));
        inks.dedup();
        Ok(inks)
    }
}

/// Pull the dominant colours out of an image (a logo, a mood photo) with
/// k-means in Lab. Transparent and near-transparent pixels are ignored.
/// Returns at most `count` colours, most used first, with their share of pixels.
pub fn extract_palette(image: &image::RgbaImage, count: usize) -> Vec<(Rgba, f32)> {
    let count = count.clamp(1, 12);
    let step = ((image.width() * image.height()) as usize / 40_000).max(1);
    let samples: Vec<(Lab, [u8; 3])> = image
        .pixels()
        .step_by(step)
        .filter(|p| p.0[3] > 200)
        .map(|p| {
            (
                Lab::from_rgb(p.0[0], p.0[1], p.0[2]),
                [p.0[0], p.0[1], p.0[2]],
            )
        })
        .collect();
    if samples.is_empty() {
        return Vec::new();
    }
    // Seed deterministically: spread across the samples sorted by lightness.
    let mut sorted: Vec<&(Lab, [u8; 3])> = samples.iter().collect();
    sorted.sort_by(|a, b| a.0.l.total_cmp(&b.0.l));
    let mut centres: Vec<Lab> = (0..count)
        .map(|i| sorted[(i * (sorted.len() - 1)) / count.max(2).saturating_sub(1).max(1)].0)
        .collect();
    let mut assignment = vec![0usize; samples.len()];
    for _ in 0..16 {
        for (i, (lab, _)) in samples.iter().enumerate() {
            assignment[i] = centres
                .iter()
                .enumerate()
                .min_by(|a, b| lab.delta_e(a.1).total_cmp(&lab.delta_e(b.1)))
                .map(|(k, _)| k)
                .unwrap_or(0);
        }
        for (k, centre) in centres.iter_mut().enumerate() {
            let members: Vec<&Lab> = samples
                .iter()
                .zip(&assignment)
                .filter(|(_, &a)| a == k)
                .map(|(s, _)| &s.0)
                .collect();
            if !members.is_empty() {
                let n = members.len() as f32;
                *centre = Lab {
                    l: members.iter().map(|m| m.l).sum::<f32>() / n,
                    a: members.iter().map(|m| m.a).sum::<f32>() / n,
                    b: members.iter().map(|m| m.b).sum::<f32>() / n,
                };
            }
        }
    }
    // Report each cluster as its most central real pixel, so the hex is a colour
    // that actually occurs in the source rather than an average nobody used.
    let mut out: Vec<(Rgba, f32)> = (0..count)
        .filter_map(|k| {
            let members: Vec<&(Lab, [u8; 3])> = samples
                .iter()
                .zip(&assignment)
                .filter(|(_, &a)| a == k)
                .map(|(s, _)| s)
                .collect();
            let best = members.iter().min_by(|a, b| {
                a.0.delta_e(&centres[k])
                    .total_cmp(&b.0.delta_e(&centres[k]))
            })?;
            Some((
                Rgba::rgb(best.1[0], best.1[1], best.1[2]),
                members.len() as f32 / samples.len() as f32,
            ))
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    // Merge clusters that landed on the same colour.
    let mut merged: Vec<(Rgba, f32)> = Vec::new();
    for (colour, share) in out {
        let lab = Lab::from_rgba(colour);
        if let Some(existing) = merged
            .iter_mut()
            .find(|(c, _)| Lab::from_rgba(*c).delta_e(&lab) < 6.0)
        {
            existing.1 += share;
        } else {
            merged.push((colour, share));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{
        "name": "Test",
        "inks": [
            {"name": "cream", "hex": "#F9ECDD", "role": "paper"},
            {"name": "ink", "hex": "#2A1308", "role": "ink"},
            {"name": "coral", "hex": "#F85647", "role": "accent"}
        ]
    }"##;

    #[test]
    fn a_brand_parses_and_orders_dark_to_light() {
        let brand = Brand::from_json(SAMPLE).unwrap();
        let inks = brand.inks_dark_to_light().unwrap();
        assert_eq!(inks.first().unwrap().to_hex().to_uppercase(), "#2A1308");
        assert_eq!(inks.last().unwrap().to_hex().to_uppercase(), "#F9ECDD");
        assert_eq!(brand.with_role(Role::Accent).len(), 1);
        assert_eq!(brand.logos.max_width, 0.22);
    }

    #[test]
    fn a_bad_hex_is_named() {
        let bad = SAMPLE.replace("#F85647", "#F8564");
        let error = Brand::from_json(&bad).unwrap_err().to_string();
        assert!(error.contains("coral"), "{error}");
    }

    #[test]
    fn palette_extraction_finds_the_two_colours_of_a_split_image() {
        let mut img = image::RgbaImage::new(40, 20);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 30 {
                image::Rgba([240, 90, 70, 255])
            } else {
                image::Rgba([20, 20, 30, 255])
            };
        }
        let palette = extract_palette(&img, 3);
        assert_eq!(palette.len(), 2, "{palette:?}");
        assert_eq!(palette[0].0, Rgba::rgb(240, 90, 70));
        assert!((palette[0].1 - 0.75).abs() < 0.02);
    }
}
