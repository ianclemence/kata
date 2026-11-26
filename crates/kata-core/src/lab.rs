//! CIE Lab, the colour space where "these two inks look alike" is a distance.
//!
//! Spot printing, palette extraction and the ink-against-ground check all ask
//! that question, so they share this one conversion (sRGB, D65 white).

use crate::color::Rgba;

/// A colour in CIE L*a*b*: lightness 0..100, then the green–red and blue–yellow axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

fn srgb_to_linear(channel: u8) -> f32 {
    let c = f32::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn pivot(t: f32) -> f32 {
    if t > 0.008_856 {
        t.cbrt()
    } else {
        7.787 * t + 16.0 / 116.0
    }
}

impl Lab {
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Lab {
        let (r, g, b) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
        // sRGB to XYZ, normalised to the D65 white point.
        let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
        let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
        let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
        let (fx, fy, fz) = (pivot(x), pivot(y), pivot(z));
        Lab {
            l: 116.0 * fy - 16.0,
            a: 500.0 * (fx - fy),
            b: 200.0 * (fy - fz),
        }
    }

    pub fn from_rgba(colour: Rgba) -> Lab {
        Lab::from_rgb(colour.r, colour.g, colour.b)
    }

    /// CIE76 distance. Around 2 is a just-noticeable difference; under 10 two
    /// flat inks read as the same colour at poster distance.
    pub fn delta_e(&self, other: &Lab) -> f32 {
        ((self.l - other.l).powi(2) + (self.a - other.a).powi(2) + (self.b - other.b).powi(2))
            .sqrt()
    }

    /// Distance with lightness and hue weighted separately. Spot printing weighs
    /// hue more than lightness so a shaded orange umbrella stays orange.
    pub fn weighted_distance(&self, other: &Lab, lightness: f32, chroma: f32) -> f32 {
        lightness * (self.l - other.l).powi(2)
            + chroma * ((self.a - other.a).powi(2) + (self.b - other.b).powi(2))
    }

    pub fn chroma(&self) -> f32 {
        (self.a * self.a + self.b * self.b).sqrt()
    }
}

/// WCAG 2 contrast ratio between two colours, 1.0 (none) to 21.0 (black on white).
pub fn contrast_ratio(a: Rgba, b: Rgba) -> f32 {
    let luminance = |c: Rgba| {
        0.2126 * srgb_to_linear(c.r) + 0.7152 * srgb_to_linear(c.g) + 0.0722 * srgb_to_linear(c.b)
    };
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_black_sit_at_the_ends_of_lightness() {
        let white = Lab::from_rgb(255, 255, 255);
        let black = Lab::from_rgb(0, 0, 0);
        assert!((white.l - 100.0).abs() < 0.1, "{white:?}");
        assert!(black.l.abs() < 0.1, "{black:?}");
        assert!(white.chroma() < 0.5);
    }

    #[test]
    fn a_red_has_positive_a() {
        let red = Lab::from_rgb(255, 0, 0);
        assert!((red.l - 53.2).abs() < 0.5, "{red:?}");
        assert!((red.a - 80.1).abs() < 1.0, "{red:?}");
    }

    #[test]
    fn contrast_ratio_matches_wcag_for_black_on_white() {
        let ratio = contrast_ratio(Rgba::BLACK, Rgba::WHITE);
        assert!((ratio - 21.0).abs() < 0.01, "{ratio}");
        assert!((contrast_ratio(Rgba::WHITE, Rgba::WHITE) - 1.0).abs() < 1e-6);
    }
}
