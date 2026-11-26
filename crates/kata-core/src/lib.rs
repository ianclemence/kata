// Channel loops (`for c in 0..3`) index two or three buffers at once and read
// more plainly than zipped iterators; `Rgba::rgba` mirrors CSS's rgba().
#![allow(clippy::needless_range_loop, clippy::self_named_constructors)]

//! Kata core: the image pipeline, the mask and sticker algorithms, and the
//! segmentation interface.
//!
//! Everything here is platform agnostic. It builds for native targets and for
//! `wasm32-unknown-unknown` from the same source, so the desktop app, the CLI
//! and the browser app all run the same maths.

pub mod brand;
pub mod color;
pub mod error;
pub mod export;
pub mod filters;
pub mod grain;
pub mod halo;
pub mod inspect;
pub mod lab;
pub mod mask;
pub mod matte;
pub mod model;
pub mod ops;
pub mod parallel;
pub mod pipeline;
pub mod posterize;
pub mod print;
pub mod segment;
pub mod sticker;
pub mod style;
pub mod targets;
pub mod texture;

pub use color::Rgba;
pub use error::{KataError, Result};
pub use mask::Mask;
pub use mask::MaskParams;
pub use segment::{Segmenter, SegmenterInfo};
pub use sticker::{render_sticker, render_sticker_preview};
pub use style::{HaloStyle, Palette, StickerStyle};

use image::RgbaImage;

/// Refuse images beyond this many pixels rather than exhausting memory.
///
/// 12 MP is the target workload; 120 MP leaves room for scanned art while a
/// corrupt header cannot ask us for an arbitrary allocation.
pub const MAX_PIXELS: u64 = 120_000_000;

/// Decode an image from bytes into RGBA8, refusing anything oversized or
/// unreadable with a message that says which.
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage> {
    let reader = || {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| KataError::Invalid(format!("cannot read the image header: {e}")))
    };

    // Check the dimensions before decoding the pixels so a hostile header
    // cannot make us allocate gigabytes.
    if let Ok((w, h)) = reader()?.into_dimensions() {
        check_dimensions(w, h)?;
    }

    let decoded = reader()?
        .decode()
        .map_err(|e| KataError::Invalid(format!("cannot decode the image: {e}")))?;
    Ok(decoded.to_rgba8())
}

/// Reject absurd dimensions before they become an allocation.
pub fn check_dimensions(width: u32, height: u32) -> Result<()> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 {
        return Err(KataError::Invalid("the image is empty".into()));
    }
    if pixels > MAX_PIXELS {
        return Err(KataError::TooLarge {
            width,
            height,
            limit: MAX_PIXELS,
        });
    }
    Ok(())
}

/// The pixel format we export. WebP is lossless here so a cutout keeps its alpha.
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    export::encode(image, &export::ExportOptions::png())
}

#[cfg(target_arch = "wasm32")]
mod wasm_init {
    use wasm_bindgen::prelude::*;

    /// Call once from JavaScript before using the module.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }
}
