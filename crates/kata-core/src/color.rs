//! Colour types. Kept tiny on purpose: everything downstream needs is an
//! RGBA word and a perceptual lightness.

use crate::error::{KataError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::rgba(0, 0, 0, 0);
    pub const BLACK: Rgba = Rgba::rgba(0, 0, 0, 255);
    pub const WHITE: Rgba = Rgba::rgba(255, 255, 255, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }

    /// Parse `#RRGGBB` or `#RRGGBBAA`, with or without the `#`.
    pub fn from_hex(text: &str) -> Result<Self> {
        let s = text.trim().trim_start_matches('#');
        let parse = |range: std::ops::Range<usize>| -> Result<u8> {
            u8::from_str_radix(&s[range], 16)
                .map_err(|_| KataError::Invalid(format!("`{text}` is not a hex colour")))
        };
        match s.len() {
            6 => Ok(Rgba::rgb(parse(0..2)?, parse(2..4)?, parse(4..6)?)),
            8 => Ok(Rgba::rgba(
                parse(0..2)?,
                parse(2..4)?,
                parse(4..6)?,
                parse(6..8)?,
            )),
            3 => {
                let n = |i: usize| -> Result<u8> {
                    let d = u8::from_str_radix(&s[i..i + 1], 16)
                        .map_err(|_| KataError::Invalid(format!("`{text}` is not a hex colour")))?;
                    Ok(d * 17)
                };
                Ok(Rgba::rgb(n(0)?, n(1)?, n(2)?))
            }
            _ => Err(KataError::Invalid(format!(
                "`{text}` is not a hex colour (expected #RRGGBB)"
            ))),
        }
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Rec. 709 relative-luma lightness, 0..=255. Used to order inks and to
    /// decide which ink a pixel belongs to.
    pub fn luma(&self) -> u8 {
        let y =
            0.2126 * f32::from(self.r) + 0.7152 * f32::from(self.g) + 0.0722 * f32::from(self.b);
        y.round().clamp(0.0, 255.0) as u8
    }

    pub fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Rgba { a, ..self }
    }
}

impl From<[u8; 4]> for Rgba {
    fn from(v: [u8; 4]) -> Self {
        Rgba::rgba(v[0], v[1], v[2], v[3])
    }
}

impl From<image::Rgba<u8>> for Rgba {
    fn from(p: image::Rgba<u8>) -> Self {
        Rgba::rgba(p.0[0], p.0[1], p.0[2], p.0[3])
    }
}

impl From<Rgba> for image::Rgba<u8> {
    fn from(c: Rgba) -> Self {
        image::Rgba(c.to_array())
    }
}

/// Source-over compositing of `src` onto `dst`, both straight (non-premultiplied).
pub fn over(dst: Rgba, src: Rgba) -> Rgba {
    if src.a == 0 {
        return dst;
    }
    if src.a == 255 || dst.a == 0 {
        // Not quite: if dst has alpha, a fully opaque src still wins.
        return src;
    }
    let sa = f32::from(src.a) / 255.0;
    let da = f32::from(dst.a) / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= f32::EPSILON {
        return Rgba::TRANSPARENT;
    }
    let mix = |s: u8, d: u8| -> u8 {
        let v = (f32::from(s) * sa + f32::from(d) * da * (1.0 - sa)) / oa;
        v.round().clamp(0.0, 255.0) as u8
    };
    Rgba::rgba(
        mix(src.r, dst.r),
        mix(src.g, dst.g),
        mix(src.b, dst.b),
        (oa * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// Blend two colours by `t` in 0..=1, ignoring alpha.
pub fn lerp_rgb(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| -> u8 {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba::rgba(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), a.a)
}
