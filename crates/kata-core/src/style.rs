//! What a sticker is: a palette, a halo, and the print artefacts. One struct so
//! the window, the CLI and the browser all speak the same JSON.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use serde::{Deserialize, Serialize};

/// The width a halo is specified against. A halo is described in pixels "at
/// 1080p"; on a larger image it scales with the long side so it looks the same.
pub const HALO_REFERENCE_SIDE: u32 = 1920;

/// The cream that katakata prints behind every sticker.
pub const DEFAULT_HALO: Rgba = Rgba::rgba(0xFB, 0xEF, 0xDC, 0xFF);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HaloStyle {
    pub color: Rgba,
    /// Width in pixels, measured against [`HALO_REFERENCE_SIDE`] when
    /// `auto_scale` is on.
    pub width: f32,
    /// Scale the width with the image, so a 12 MP photo gets a proportionally
    /// thicker outline instead of a hairline.
    pub auto_scale: bool,
    /// Soft edge strength 0..=100. 0 is a hard cut.
    pub softness: u8,
    /// The small offset that makes the outline look hand-cut.
    pub offset: (i32, i32),
}

impl Default for HaloStyle {
    fn default() -> Self {
        HaloStyle {
            color: DEFAULT_HALO,
            width: 18.0,
            auto_scale: true,
            softness: 35,
            offset: (4, 3),
        }
    }
}

impl HaloStyle {
    /// The width to draw at, in whole pixels, for an image of `long_side`.
    pub fn effective_width(&self, long_side: u32) -> u32 {
        if self.width <= 0.0 {
            return 0;
        }
        let scaled = if self.auto_scale {
            self.width * long_side as f32 / HALO_REFERENCE_SIDE as f32
        } else {
            self.width
        };
        scaled.round().clamp(0.0, 512.0) as u32
    }
}

/// The ink sets. Every ramp is ordered dark to light and stays in the warm
/// paper-and-ink family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Palette {
    AfroRetro,
    Riso,
    Seventies,
    Mono,
    Custom(Vec<Rgba>),
}

impl Default for Palette {
    fn default() -> Self {
        Palette::AfroRetro
    }
}

impl Palette {
    pub fn name(&self) -> &'static str {
        match self {
            Palette::AfroRetro => "afro",
            Palette::Riso => "riso",
            Palette::Seventies => "70s",
            Palette::Mono => "mono",
            Palette::Custom(_) => "custom",
        }
    }

    /// Every ink in the ramp, dark to light.
    pub fn all_inks(&self) -> Vec<Rgba> {
        match self {
            Palette::AfroRetro => vec![
                Rgba::rgb(0x1B, 0x1A, 0x17), // ink
                Rgba::rgb(0x5E, 0x24, 0x1B), // oxblood
                Rgba::rgb(0xB5, 0x4A, 0x24), // burnt orange
                Rgba::rgb(0xE0, 0x9C, 0x3E), // ochre
                Rgba::rgb(0xF4, 0xE6, 0xCB), // paper
            ],
            Palette::Riso => vec![
                Rgba::rgb(0x1E, 0x1B, 0x2E), // riso black
                Rgba::rgb(0x2C, 0x4C, 0x8F), // federal blue
                Rgba::rgb(0xE0, 0x47, 0x5B), // fluorescent pink
                Rgba::rgb(0xF2, 0xB0, 0x41), // yellow
                Rgba::rgb(0xF7, 0xEF, 0xE2), // stock
            ],
            Palette::Seventies => vec![
                Rgba::rgb(0x2B, 0x21, 0x18), // dark brown
                Rgba::rgb(0x7A, 0x3B, 0x2E), // terracotta
                Rgba::rgb(0xB9, 0x7A, 0x3C), // mustard
                Rgba::rgb(0xD9, 0xBE, 0x8E), // sand
                Rgba::rgb(0xF3, 0xE8, 0xD3), // bone
            ],
            Palette::Mono => vec![
                Rgba::rgb(0x14, 0x11, 0x0F),
                Rgba::rgb(0x46, 0x41, 0x3B),
                Rgba::rgb(0x87, 0x80, 0x76),
                Rgba::rgb(0xC4, 0xBD, 0xB1),
                Rgba::rgb(0xF5, 0xF1, 0xE8),
            ],
            Palette::Custom(inks) => {
                let mut inks = inks.clone();
                inks.sort_by_key(Rgba::luma);
                inks
            }
        }
    }

    /// Pick `count` inks out of the ramp, spread evenly from the darkest to the
    /// lightest so that two inks means ink and paper, not two mid-tones.
    pub fn resolve(&self, count: usize) -> Result<Vec<Rgba>> {
        let count = count.clamp(2, 5);
        let all = self.all_inks();
        if all.is_empty() {
            return Err(KataError::Invalid("the palette has no inks".into()));
        }
        if all.len() < count {
            return Err(KataError::Invalid(format!(
                "{} has {} inks but {count} were requested",
                self.name(),
                all.len()
            )));
        }
        let mut sorted = all;
        sorted.sort_by_key(Rgba::luma);
        let last = sorted.len() - 1;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let position = if count == 1 {
                0
            } else {
                (i * last) / (count - 1)
            };
            out.push(sorted[position]);
        }
        Ok(out)
    }

    /// The named palettes, for a menu or a CLI help string.
    pub fn presets() -> [Palette; 4] {
        [
            Palette::AfroRetro,
            Palette::Riso,
            Palette::Seventies,
            Palette::Mono,
        ]
    }

    /// Parse `afro`, `riso`, `70s`, `mono`, or a comma-separated ink list.
    pub fn parse(text: &str) -> Result<Palette> {
        let lower = text.trim().to_ascii_lowercase();
        match lower.as_str() {
            "afro" | "afro-retro" | "afroretro" => Ok(Palette::AfroRetro),
            "riso" | "risograph" => Ok(Palette::Riso),
            "70s" | "70" | "seventies" | "seventy" => Ok(Palette::Seventies),
            "mono" | "monochrome" | "gray" | "grey" => Ok(Palette::Mono),
            _ => {
                let inks: Result<Vec<Rgba>> =
                    text.split(',').map(|part| Rgba::from_hex(part.trim())).collect();
                let inks = inks?;
                if inks.len() < 2 {
                    return Err(KataError::Invalid(
                        "a custom palette needs at least two hex colours".into(),
                    ));
                }
                Ok(Palette::Custom(inks))
            }
        }
    }
}

/// Everything that decides what a sticker looks like.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StickerStyle {
    /// 2 to 5 flat inks.
    pub inks: usize,
    pub palette: Palette,
    pub halo: HaloStyle,
    /// Offset the darkest ink by 0..=3 px.
    pub registration: u8,
    /// Paper grain 0..=100.
    pub grain: u8,
    /// Halftone dots 0..=100.
    pub halftone: u8,
    /// Bilateral smoothing 0..=100.
    pub smooth: u8,
    /// CLAHE strength 0..=100.
    pub clahe: u8,
    /// Fraction of highlights to blow out, 0.0..=0.2.
    pub highlight_clip: f32,
    /// Median radius on the ink map: 0, 1 or 2.
    pub median_radius: u32,
}

impl Default for StickerStyle {
    fn default() -> Self {
        StickerStyle {
            inks: 4,
            palette: Palette::default(),
            halo: HaloStyle::default(),
            registration: 1,
            grain: 22,
            halftone: 0,
            smooth: 60,
            clahe: 55,
            highlight_clip: 0.06,
            median_radius: 1,
        }
    }
}

impl StickerStyle {
    /// The named presets, in the order the style menu shows them.
    pub const PRESET_NAMES: [&'static str; 4] = ["afro", "riso", "70s", "mono"];

    /// A preset by name, or `None`.
    pub fn preset(name: &str) -> Option<StickerStyle> {
        let palette = Palette::parse(name).ok()?;
        let inks = match &palette {
            Palette::AfroRetro => 4,
            Palette::Riso => 4,
            Palette::Seventies => 5,
            Palette::Mono => 3,
            Palette::Custom(_) => return None,
        };
        let mut style = StickerStyle {
            palette,
            inks,
            ..StickerStyle::default()
        };
        style.halo = HaloStyle::default();
        Some(style)
    }

    pub fn palette_inks(&self) -> Result<Vec<Rgba>> {
        self.palette.resolve(self.inks)
    }

    /// Validate before any pixels are touched, so a bad style is a clear error
    /// rather than a strange-looking image.
    pub fn validate(&self) -> Result<()> {
        if !(2..=5).contains(&self.inks) {
            return Err(KataError::Invalid(format!(
                "inks must be between 2 and 5, not {}",
                self.inks
            )));
        }
        self.palette_inks().map(|_| ())?;
        if self.halo.width < 0.0 {
            return Err(KataError::Invalid("the halo width cannot be negative".into()));
        }
        if !self.highlight_clip.is_finite() || !(0.0..=0.2).contains(&self.highlight_clip) {
            return Err(KataError::Invalid(
                "highlight_clip must be between 0 and 0.2".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_are_ordered_dark_to_light() {
        for palette in Palette::presets() {
            let inks = palette.all_inks();
            let lumas: Vec<u8> = inks.iter().map(Rgba::luma).collect();
            let mut sorted = lumas.clone();
            sorted.sort_unstable();
            assert_eq!(lumas, sorted, "{} is out of order", palette.name());
        }
    }

    #[test]
    fn resolving_two_inks_gives_the_extremes() {
        let inks = Palette::AfroRetro.resolve(2).unwrap();
        assert_eq!(inks.len(), 2);
        assert_eq!(inks[0], Palette::AfroRetro.all_inks()[0]);
        assert_eq!(inks[1].luma(), Palette::AfroRetro.all_inks()[4].luma());
    }

    #[test]
    fn resolving_five_inks_gives_the_whole_ramp() {
        assert_eq!(Palette::Riso.resolve(5).unwrap().len(), 5);
    }

    #[test]
    fn inks_are_clamped_to_the_printable_range() {
        assert_eq!(Palette::Mono.resolve(1).unwrap().len(), 2);
        assert_eq!(Palette::Mono.resolve(99).unwrap().len(), 5);
    }

    #[test]
    fn halo_width_scales_with_the_image() {
        let halo = HaloStyle::default();
        assert_eq!(halo.effective_width(1920), 18);
        assert_eq!(halo.effective_width(3840), 36);
        assert_eq!(halo.effective_width(960), 9);
    }

    #[test]
    fn halo_can_be_pinned_to_pixels() {
        let halo = HaloStyle {
            auto_scale: false,
            ..Default::default()
        };
        assert_eq!(halo.effective_width(4000), 18);
    }

    #[test]
    fn a_zero_width_halo_draws_nothing() {
        let halo = HaloStyle {
            width: 0.0,
            ..Default::default()
        };
        assert_eq!(halo.effective_width(2000), 0);
    }

    #[test]
    fn parse_accepts_names_and_hex_lists() {
        assert_eq!(Palette::parse("afro").unwrap(), Palette::AfroRetro);
        assert_eq!(Palette::parse("70s").unwrap(), Palette::Seventies);
        let custom = Palette::parse("#000000, #FFFFFF").unwrap();
        assert_eq!(custom.all_inks().len(), 2);
        assert!(Palette::parse("nonsense").is_err());
    }

    #[test]
    fn validate_rejects_impossible_styles() {
        let style = StickerStyle {
            inks: 9,
            ..Default::default()
        };
        assert!(style.validate().is_err());
        let style = StickerStyle {
            highlight_clip: 0.9,
            ..Default::default()
        };
        assert!(style.validate().is_err());
    }

    #[test]
    fn presets_resolve_to_their_ink_counts() {
        for name in StickerStyle::PRESET_NAMES {
            let style = StickerStyle::preset(name).unwrap();
            assert_eq!(
                style.palette_inks().unwrap().len(),
                style.inks,
                "preset {name}"
            );
        }
    }
}
