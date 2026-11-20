//! The native ONNX Runtime backend.
//!
//! Kept behind the `onnx` feature so the core still builds for WebAssembly and
//! so `cargo test` needs no runtime binaries.

#![cfg(feature = "onnx")]

use crate::error::{KataError, Result};
use crate::mask::Mask;
use crate::model::ModelSpec;
use crate::segment::{
    logits_to_mask, to_tensor_input, Backend, Progress, Segmenter, SegmenterInfo, MODEL_INPUT_SIZE,
};
use image::RgbaImage;

pub struct OnnxSegmenter;
