//! Getting a sticker out of the app: format, crop, scale, naming, sheets.

use crate::color::Rgba;
use crate::error::{KataError, Result};
use crate::filters;
use crate::halo::flatten;
use crate::mask::Mask;
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Webp,
}

impl Format {
    pub fn extension(&self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Webp => "webp",
        }
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Webp => "image/webp",
        }
    }

    pub fn parse(text: &str) -> Result<Format> {
        match text.trim().to_ascii_lowercase().as_str() {
            "png" => Ok(Format::Png),
            "webp" => Ok(Format::Webp),
            other => Err(KataError::Invalid(format!(
                "`{other}` is not an export format (png or webp)"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportOptions {
    pub format: Format,
    /// Fill the transparent area with this colour. `None` keeps the alpha.
    pub fill: Option<Rgba>,
    /// Crop to the subject, so a batch export is a set of tight stickers.
    pub crop: bool,
    /// Padding around the crop, as a fraction of the subject's long side.
    pub padding: f32,
    /// 1, 2 or 3. Applied after cropping.
    pub scale: u8,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            format: Format::Png,
            fill: None,
            crop: true,
            padding: 0.06,
            scale: 1,
        }
    }
}

impl ExportOptions {
    pub fn png() -> Self {
        ExportOptions::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !(1..=3).contains(&self.scale) {
            return Err(KataError::Invalid(format!(
                "scale must be 1, 2 or 3, not {}",
                self.scale
            )));
        }
        if !(0.0..=1.0).contains(&self.padding) {
            return Err(KataError::Invalid(
                "padding must be a fraction between 0 and 1".into(),
            ));
        }
        Ok(())
    }
}

/// Crop to the visible subject, with room around it for the halo.
pub fn crop_to_subject(image: &RgbaImage, padding: f32) -> RgbaImage {
    let mask = Mask::from_alpha(image);
    let Some((x0, y0, x1, y1)) = mask.bbox() else {
        return image.clone();
    };
    let (width, height) = image.dimensions();
    let subject_w = x1 - x0 + 1;
    let subject_h = y1 - y0 + 1;
    let pad = (subject_w.max(subject_h) as f32 * padding.clamp(0.0, 1.0)).round() as i64;

    let left = (x0 as i64 - pad).max(0) as u32;
    let top = (y0 as i64 - pad).max(0) as u32;
    let right = ((x1 as i64 + pad).min(width as i64 - 1)) as u32;
    let bottom = ((y1 as i64 + pad).min(height as i64 - 1)) as u32;

    image::imageops::crop_imm(image, left, top, right - left + 1, bottom - top + 1).to_image()
}

/// Everything that happens to a sticker between "rendered" and "bytes".
pub fn prepare(image: &RgbaImage, options: &ExportOptions) -> Result<RgbaImage> {
    options.validate()?;
    let mut out = if options.crop {
        crop_to_subject(image, options.padding)
    } else {
        image.clone()
    };
    if let Some(fill) = options.fill {
        out = flatten(&out, fill);
    }
    if options.scale > 1 {
        let factor = u32::from(options.scale);
        out = filters::resize(&out, out.width() * factor, out.height() * factor);
    }
    Ok(out)
}

/// Encode a rendered sticker to bytes.
pub fn encode(image: &RgbaImage, options: &ExportOptions) -> Result<Vec<u8>> {
    let prepared = prepare(image, options)?;
    let format = match options.format {
        Format::Png => ImageFormat::Png,
        Format::Webp => ImageFormat::WebP,
    };
    let mut buffer = Vec::new();
    image::DynamicImage::ImageRgba8(prepared)
        .write_to(&mut Cursor::new(&mut buffer), format)
        .map_err(KataError::Image)?;
    Ok(buffer)
}

/// Write the file, refusing to overwrite something that is already there
/// unless `overwrite` is set. Originals are never silently replaced.
pub fn write_file(path: &std::path::Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    if !overwrite && path.exists() {
        return Err(KataError::Invalid(format!(
            "{} already exists (use --overwrite to replace it)",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Expand an output-name pattern. Tokens: `{name}`, `{index}`, `{style}`,
/// `{inks}`, `{width}`, `{height}`. The extension is added by the caller.
pub fn expand_pattern(
    pattern: &str,
    name: &str,
    index: usize,
    style: &str,
    inks: usize,
    width: u32,
    height: u32,
) -> String {
    let mut stem = pattern
        .replace("{name}", name)
        .replace("{index}", &format!("{index:03}"))
        .replace("{style}", style)
        .replace("{inks}", &inks.to_string())
        .replace("{width}", &width.to_string())
        .replace("{height}", &height.to_string());
    if stem.trim().is_empty() {
        stem = name.to_string();
    }
    stem
}

/// One sheet of stickers: a grid on a background, each cell holding one cutout
/// scaled to fit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetOptions {
    pub columns: u32,
    /// Width of one cell in pixels; the sheet grows to fit the grid.
    pub cell: u32,
    pub gap: u32,
    pub background: Rgba,
    /// Room around each sticker inside its cell, as a fraction.
    pub padding: f32,
}

impl Default for SheetOptions {
    fn default() -> Self {
        SheetOptions {
            columns: 3,
            cell: 512,
            gap: 24,
            background: Rgba::rgba(0xF4, 0xE6, 0xCB, 0xFF),
            padding: 0.06,
        }
    }
}

pub fn sticker_sheet(items: &[RgbaImage], options: &SheetOptions) -> Result<RgbaImage> {
    if items.is_empty() {
        return Err(KataError::Invalid("the sheet has no stickers".into()));
    }
    let columns = options.columns.clamp(1, 12);
    let rows = (items.len() as u32).div_ceil(columns);
    let gap = options.gap;
    let width = columns * options.cell + (columns + 1) * gap;
    let height = rows * options.cell + (rows + 1) * gap;
    let mut sheet = RgbaImage::from_pixel(
        width,
        height,
        image::Rgba(options.background.with_alpha(255).to_array()),
    );

    for (index, item) in items.iter().enumerate() {
        let col = index as u32 % columns;
        let row = index as u32 / columns;
        let cell_x = gap + col * (options.cell + gap);
        let cell_y = gap + row * (options.cell + gap);

        let inner = (options.cell as f32 * (1.0 - 2.0 * options.padding.clamp(0.0, 0.4))).max(1.0);
        let longest = item.width().max(item.height()).max(1) as f32;
        let scale = inner / longest;
        let target_w = ((item.width() as f32 * scale).round() as u32).max(1);
        let target_h = ((item.height() as f32 * scale).round() as u32).max(1);
        let scaled = filters::resize(item, target_w, target_h);

        let offset_x = cell_x + (options.cell - target_w) / 2;
        let offset_y = cell_y + (options.cell - target_h) / 2;
        for (x, y, pixel) in scaled.enumerate_pixels() {
            let tx = offset_x + x;
            let ty = offset_y + y;
            if tx >= width || ty >= height {
                continue;
            }
            if pixel.0[3] == 0 {
                continue;
            }
            let base = sheet.get_pixel(tx, ty);
            let blended = crate::color::over(Rgba::from(*base), Rgba::from(*pixel));
            sheet.put_pixel(tx, ty, image::Rgba(blended.to_array()));
        }
    }
    Ok(sheet)
}

/// A file name that does not collide with one already on disk.
pub fn unique_path(path: &std::path::Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "sticker".into());
    let extension = path
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
    for n in 2..10_000 {
        let candidate = if extension.is_empty() {
            parent.join(format!("{stem} ({n})"))
        } else {
            parent.join(format!("{stem} ({n}).{extension}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba as Pixel;

    #[test]
    fn crop_tightens_to_the_subject_with_padding() {
        let mut img = RgbaImage::new(100, 100);
        for y in 40..60 {
            for x in 40..60 {
                img.put_pixel(x, y, Pixel([255, 0, 0, 255]));
            }
        }
        let cropped = crop_to_subject(&img, 0.0);
        assert_eq!((cropped.width(), cropped.height()), (20, 20));
        let padded = crop_to_subject(&img, 0.5);
        assert_eq!((padded.width(), padded.height()), (40, 40));
    }

    #[test]
    fn crop_of_a_blank_image_returns_it_unchanged() {
        let img = RgbaImage::new(10, 10);
        assert_eq!(crop_to_subject(&img, 0.1), img);
    }

    #[test]
    fn scale_multiplies_the_size() {
        let img = RgbaImage::from_pixel(10, 10, Pixel([1, 2, 3, 255]));
        let options = ExportOptions {
            crop: false,
            scale: 2,
            ..Default::default()
        };
        let out = prepare(&img, &options).unwrap();
        assert_eq!((out.width(), out.height()), (20, 20));
    }

    #[test]
    fn fill_removes_transparency() {
        let img = RgbaImage::new(4, 4);
        let options = ExportOptions {
            crop: false,
            fill: Some(Rgba::rgb(10, 20, 30)),
            ..Default::default()
        };
        let out = prepare(&img, &options).unwrap();
        assert_eq!(out.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }

    #[test]
    fn invalid_scale_and_padding_are_rejected() {
        let img = RgbaImage::new(4, 4);
        let options = ExportOptions {
            scale: 4,
            ..Default::default()
        };
        assert!(prepare(&img, &options).is_err());
        let options = ExportOptions {
            padding: 1.5,
            ..Default::default()
        };
        assert!(prepare(&img, &options).is_err());
    }

    #[test]
    fn png_encoding_round_trips() {
        let mut img = RgbaImage::new(8, 8);
        img.put_pixel(4, 4, Pixel([1, 2, 3, 255]));
        // The round trip must keep the canvas and its transparent background;
        // cropping to the subject is an explicit option with its own test.
        let options = ExportOptions {
            crop: false,
            ..Default::default()
        };
        let bytes = encode(&img, &options).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(7, 7).0[3], 0);
        assert_eq!(decoded.get_pixel(4, 4).0, [1, 2, 3, 255]);
    }

    #[test]
    fn webp_encoding_produces_a_webp() {
        let img = RgbaImage::from_pixel(8, 8, Pixel([200, 10, 10, 255]));
        let options = ExportOptions {
            format: Format::Webp,
            ..Default::default()
        };
        let bytes = encode(&img, &options).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }

    #[test]
    fn pattern_expands_its_tokens() {
        let name = expand_pattern(
            "{name}-{index}-{style}-{inks}ink",
            "portrait",
            7,
            "afro",
            4,
            10,
            10,
        );
        assert_eq!(name, "portrait-007-afro-4ink");
    }

    #[test]
    fn an_empty_pattern_falls_back_to_the_name() {
        assert_eq!(
            expand_pattern("{nope}", "beach", 0, "riso", 3, 1, 1),
            "{nope}"
        );
        assert_eq!(expand_pattern("   ", "beach", 0, "riso", 3, 1, 1), "beach");
    }

    #[test]
    fn a_sheet_lays_items_out_in_a_grid() {
        let items = vec![
            RgbaImage::from_pixel(50, 50, Pixel([255, 0, 0, 255])),
            RgbaImage::from_pixel(50, 50, Pixel([0, 255, 0, 255])),
            RgbaImage::from_pixel(50, 50, Pixel([0, 0, 255, 255])),
        ];
        let options = SheetOptions {
            columns: 2,
            cell: 64,
            gap: 8,
            ..Default::default()
        };
        let sheet = sticker_sheet(&items, &options).unwrap();
        assert_eq!(sheet.width(), 2 * 64 + 3 * 8);
        assert_eq!(sheet.height(), 2 * 64 + 3 * 8);
        // The background colour shows in the gap.
        assert_eq!(sheet.get_pixel(0, 0).0, options.background.to_array());
    }

    #[test]
    fn an_empty_sheet_is_an_error() {
        assert!(sticker_sheet(&[], &SheetOptions::default()).is_err());
    }

    #[test]
    fn unique_path_does_not_collide() {
        let dir = std::env::temp_dir().join("kata-unique-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.png");
        std::fs::write(&path, b"x").unwrap();
        let second = unique_path(&path);
        assert_ne!(second, path);
        assert!(second.to_string_lossy().contains("(2)"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
