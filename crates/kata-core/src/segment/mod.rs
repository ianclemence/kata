//! The segmentation interface.
//!
//! Everything that removes a background sits behind [`Segmenter`]. The desktop
//! app drives ONNX Runtime through the `ort` crate; the browser drives ONNX
//! Runtime Web through `wasm-bindgen`. Both hand back the same [`Mask`], so the
//! refine panel, the style pass and the export path are identical.

pub mod onnx;

use crate::error::{KataError, Result};
use crate::mask::Mask;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// Side length the matting models expect, in pixels. Both BiRefNet variants are
/// exported at 1024; the image is letterboxed to preserve aspect ratio rather
/// than squashed.
pub const MODEL_INPUT_SIZE: u32 = 1024;

/// ImageNet statistics, which is what the BiRefNet exports were trained with.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// A hardware execution provider: DirectML, CoreML or CUDA.
    Gpu,
    /// ONNX Runtime on the CPU.
    Cpu,
    /// ONNX Runtime Web in the browser.
    Web,
}

impl Backend {
    pub fn label(&self) -> &'static str {
        match self {
            Backend::Gpu => "GPU",
            Backend::Cpu => "CPU",
            Backend::Web => "Web",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmenterInfo {
    pub model_id: String,
    pub model_name: String,
    pub backend: Backend,
    /// Name of the execution provider, e.g. "DirectML", "CoreML", "CPUExecutionProvider".
    pub provider: String,
}

/// Progress in 0.0..=1.0, reported so a batch queue can show per-item state.
pub type Progress<'a> = &'a mut dyn FnMut(f32);

pub trait Segmenter: Send + Sync {
    fn info(&self) -> SegmenterInfo;

    /// Produce a mask the same size as `image`.
    fn segment(&self, image: &RgbaImage, progress: Progress<'_>) -> Result<Mask>;
}

/// Letterbox an image into the model's input tensor: NCHW, normalised, padded
/// with the mean colour so the pad neither reads as background nor as subject.
///
/// Returns the tensor and the scale and offset needed to undo the letterbox.
pub fn to_tensor_input(image: &RgbaImage, size: u32) -> (Vec<f32>, f32, u32, u32) {
    let (width, height) = image.dimensions();
    let scale = (size as f32 / width as f32).min(size as f32 / height as f32);
    let scaled_w = ((width as f32 * scale).round() as u32).clamp(1, size);
    let scaled_h = ((height as f32 * scale).round() as u32).clamp(1, size);
    let offset_x = (size - scaled_w) / 2;
    let offset_y = (size - scaled_h) / 2;

    let plane = (size * size) as usize;
    // Padding is the mean colour; normalised that is (mean - mean) / std = 0.
    let mut tensor = vec![0f32; plane * 3];

    for y in 0..scaled_h {
        let sy = (y as f32 / scale).floor() as u32;
        let sy = sy.min(height - 1);
        for x in 0..scaled_w {
            let sx = ((x as f32 / scale).floor() as u32).min(width - 1);
            let pixel = image.get_pixel(sx, sy).0;
            let index = ((offset_y + y) * size + (offset_x + x)) as usize;
            for c in 0..3 {
                let value = f32::from(pixel[c]) / 255.0;
                tensor[c * plane + index] = (value - MEAN[c]) / STD[c];
            }
        }
    }
    (tensor, scale, offset_x, offset_y)
}

/// Turn a model's output plane into a mask at the original image size.
///
/// The model returns logits; sigmoid puts them in 0..1. Then the letterbox is
/// undone with a crop and the result is resampled to the source resolution.
pub fn logits_to_mask(
    logits: &[f32],
    width: u32,
    height: u32,
    output_size: u32,
    scale: f32,
    offset_x: u32,
    offset_y: u32,
) -> Result<Mask> {
    if logits.len() != (output_size * output_size) as usize {
        return Err(KataError::Segmentation(format!(
            "the model returned {} values, expected {}x{}",
            logits.len(),
            output_size,
            output_size
        )));
    }
    let mut mask = Mask::new(width, height, 0);
    for y in 0..height {
        // Map the destination pixel back through the letterbox.
        let sy = (y as f32 * scale).floor() as i64;
        let sy = (sy + i64::from(offset_y)).clamp(0, output_size as i64 - 1) as u32;
        for x in 0..width {
            let sx = (x as f32 * scale).floor() as i64;
            let sx = (sx + i64::from(offset_x)).clamp(0, output_size as i64 - 1) as u32;
            let logit = logits[(sy * output_size + sx) as usize];
            let probability = sigmoid(logit);
            mask.set(x, y, (probability * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    Ok(mask)
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// A deliberately simple segmenter: it splits on lightness and keeps the darker
/// half. It exists so the pipeline, the CLI and the tests can run end to end
/// without a 100 MB download, and it is never presented as a matting model.
///
/// The app does not offer it. If you see it in a menu, that is a bug.
#[derive(Clone, Debug, Default)]
pub struct LumaSegmenter {
    pub threshold: u8,
}

impl Segmenter for LumaSegmenter {
    fn info(&self) -> SegmenterInfo {
        SegmenterInfo {
            model_id: "luma".into(),
            model_name: "Lightness split".into(),
            backend: Backend::Cpu,
            provider: "none".into(),
        }
    }

    fn segment(&self, image: &RgbaImage, progress: Progress<'_>) -> Result<Mask> {
        progress(0.0);
        let luma = crate::filters::to_luma(image);
        let (width, height) = luma.dimensions();
        let mut mask = Mask::new(width, height, 0);
        let cut = if self.threshold == 0 {
            128
        } else {
            self.threshold
        };
        for y in 0..height {
            for x in 0..width {
                let value = if luma.get_pixel(x, y).0[0] < cut {
                    255
                } else {
                    0
                };
                mask.set(x, y, value);
            }
        }
        progress(1.0);
        Ok(mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba as Pixel;

    #[test]
    fn tensor_input_is_nchw_and_normalised() {
        let img = RgbaImage::from_pixel(4, 4, Pixel([255, 255, 255, 255]));
        let (tensor, _, _, _) = to_tensor_input(&img, 8);
        assert_eq!(tensor.len(), 8 * 8 * 3);
        // White maps to (1 - mean) / std.
        // Channel 2 is the blue plane, so white there normalises against MEAN[2].
        let expected = (1.0 - MEAN[2]) / STD[2];
        assert!((tensor[2 * 64 + 27] - expected).abs() < 1e-4);
        // The red plane keeps its own normalisation; the planes are not copies.
        let red = (1.0 - MEAN[0]) / STD[0];
        assert!((tensor[27] - red).abs() < 1e-4);
    }

    #[test]
    fn letterbox_pads_with_the_mean_not_with_black() {
        // A wide image into a square input leaves padding top and bottom.
        let img = RgbaImage::from_pixel(8, 2, Pixel([128, 128, 128, 255]));
        let (tensor, scale, offset_x, offset_y) = to_tensor_input(&img, 8);
        assert_eq!(offset_x, 0);
        assert!(offset_y > 0);
        assert!((scale - 1.0).abs() < 1e-6);
        // The very first row is padding, so it holds the normalised mean.
        assert!(tensor[0].abs() < 1e-6, "padding was {}", tensor[0]);
    }

    #[test]
    fn sigmoid_is_stable_at_the_extremes() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(1000.0) > 0.99);
        assert!(sigmoid(-1000.0) < 0.01);
        assert!(sigmoid(-1000.0) >= 0.0);
    }

    #[test]
    fn logits_become_a_mask_at_the_source_size() {
        let logits = vec![0.0f32; 4 * 4];
        let mask = logits_to_mask(&logits, 10, 20, 4, 4.0 / 10.0, 0, 2).unwrap();
        assert_eq!((mask.width, mask.height), (10, 20));
        // A zero logit is a probability of 0.5.
        assert!((i32::from(mask.get(5, 10)) - 128).abs() <= 1);
    }

    #[test]
    fn a_wrong_output_size_is_reported() {
        assert!(logits_to_mask(&[0.0; 7], 10, 10, 4, 1.0, 0, 0).is_err());
    }

    #[test]
    fn luma_segmenter_keeps_the_dark_half() {
        let mut img = RgbaImage::new(4, 1);
        img.put_pixel(0, 0, Pixel([0, 0, 0, 255]));
        img.put_pixel(1, 0, Pixel([10, 10, 10, 255]));
        img.put_pixel(2, 0, Pixel([240, 240, 240, 255]));
        img.put_pixel(3, 0, Pixel([255, 255, 255, 255]));
        let mask = LumaSegmenter::default().segment(&img, &mut |_| {}).unwrap();
        assert_eq!(mask.get(0, 0), 255);
        assert_eq!(mask.get(1, 0), 255);
        assert_eq!(mask.get(2, 0), 0);
    }
}
