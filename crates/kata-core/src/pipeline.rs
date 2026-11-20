//! The shared pipeline, composed from the pieces so the CLI and the apps run the
//! same maths in the same order. Nothing here re-implements an algorithm.

use crate::error::Result;
use crate::export::{self, ExportOptions};
use crate::mask::{Mask, MaskParams};
use crate::segment::Segmenter;
use crate::sticker;
use crate::style::StickerStyle;
use image::RgbaImage;

/// Segment a decoded image with whatever backend the caller chose.
pub fn segment(source: &RgbaImage, segmenter: &dyn Segmenter) -> Result<Mask> {
    segmenter.segment(source, &mut |_| {})
}

/// The cutout on its own: the photo with the refined mask as its alpha channel.
pub fn cutout(source: &RgbaImage, mask: &Mask) -> Result<RgbaImage> {
    sticker::cutout(source, mask, &MaskParams::default())
}

/// Decode, segment and cut out, then encode a PNG that keeps its alpha.
pub fn cutout_png(bytes: &[u8], segmenter: &dyn Segmenter) -> Result<Vec<u8>> {
    let source = crate::decode_image(bytes)?;
    let mask = segment(&source, segmenter)?;
    let cut = cutout(&source, &mask)?;
    export::encode(&cut, &ExportOptions::png())
}

/// Decode, segment and render the default screen-print sticker as a PNG.
pub fn sticker_png(bytes: &[u8], segmenter: &dyn Segmenter) -> Result<Vec<u8>> {
    let source = crate::decode_image(bytes)?;
    let mask = segment(&source, segmenter)?;
    let sticker = sticker::render_sticker(
        &source,
        &mask,
        &MaskParams::default(),
        &StickerStyle::default(),
    )?;
    export::encode(&sticker, &ExportOptions::png())
}
