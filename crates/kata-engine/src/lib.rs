//! The native half of Kata.
//!
//! `kata-core` holds the algorithms and builds for WebAssembly. This crate is
//! what a desktop needs on top: ONNX Runtime, model downloads, the mask cache,
//! and poster composition with resvg. The CLI, the local web UI and the MCP
//! server all call into here, so they behave the same.

pub mod compose;
pub mod cut;
pub mod models;
pub mod onnx;
pub mod review;
pub mod runtime;

use kata_core::error::{KataError, Result};
use std::path::Path;

/// Read an image file and the SHA-256 of its bytes (the cache key).
pub fn open_image(path: &Path) -> Result<(image::RgbaImage, String)> {
    let bytes = std::fs::read(path)
        .map_err(|e| KataError::Invalid(format!("cannot read {}: {e}", path.display())))?;
    let hash = kata_core::model::sha256_hex(&bytes);
    Ok((kata_core::decode_image(&bytes)?, hash))
}

/// PNG of the whole canvas. Unlike `kata_core::encode_png` it never crops to
/// the subject: masks, prints and cutouts must stay aligned with their photo.
pub fn encode_full(image: &image::RgbaImage) -> Result<Vec<u8>> {
    let options = kata_core::export::ExportOptions {
        crop: false,
        ..kata_core::export::ExportOptions::png()
    };
    kata_core::export::encode(image, &options)
}

/// Save an image as PNG (full canvas), making the folder if needed.
pub fn save_png(image: &image::RgbaImage, path: &Path) -> Result<u64> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let bytes = encode_full(image)?;
    std::fs::write(path, &bytes)?;
    Ok(bytes.len() as u64)
}
