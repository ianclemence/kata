//! BiRefNet (or any single-input matting model) through ONNX Runtime.

use crate::runtime;
use image::RgbaImage;
use kata_core::error::{KataError, Result};
use kata_core::mask::Mask;
use kata_core::segment::{
    logits_to_mask, to_tensor_input, Backend, Progress, Segmenter, SegmenterInfo,
};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

pub struct OnnxSegmenter {
    session: Mutex<Session>,
    info: SegmenterInfo,
    size: u32,
    half_input: bool,
}

fn err(e: impl std::fmt::Display) -> KataError {
    KataError::Segmentation(e.to_string())
}

impl OnnxSegmenter {
    /// Open a model file. `id` and `name` are what reports and the MCP tools show.
    pub fn load(path: &Path, id: &str, name: &str) -> Result<OnnxSegmenter> {
        if !path.is_file() {
            return Err(KataError::Model(format!(
                "there is no model file at {}",
                path.display()
            )));
        }
        let env = runtime::environment()?;
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let session = Session::builder(&env)
            .map_err(err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(err)?
            .with_intra_threads(threads)
            .map_err(err)?
            .commit_from_file(path)
            .map_err(|e| {
                KataError::Model(format!(
                    "could not open {} as an ONNX model: {e}",
                    path.display()
                ))
            })?;
        if session.inputs().len() != 1 {
            return Err(KataError::Model(format!(
                "{} has {} inputs; Kata expects a single-image matting model",
                path.display(),
                session.inputs().len()
            )));
        }
        let dtype = format!("{:?}", session.inputs()[0].dtype());
        let half_input = dtype.contains("Float16");
        // Most matting exports are fixed at 1024 square; read it if the model says.
        let size = dtype
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse::<u32>().ok())
            .rfind(|n| (256..=2048).contains(n))
            .unwrap_or(kata_core::segment::MODEL_INPUT_SIZE);
        Ok(OnnxSegmenter {
            session: Mutex::new(session),
            info: SegmenterInfo {
                model_id: id.into(),
                model_name: name.into(),
                backend: Backend::Cpu,
                provider: "CPUExecutionProvider".into(),
            },
            size,
            half_input,
        })
    }

    pub fn input_size(&self) -> u32 {
        self.size
    }
}

impl Segmenter for OnnxSegmenter {
    fn info(&self) -> SegmenterInfo {
        self.info.clone()
    }

    fn segment(&self, image: &RgbaImage, progress: Progress<'_>) -> Result<Mask> {
        let (width, height) = image.dimensions();
        let size = self.size;
        let (data, scale, offset_x, offset_y) = to_tensor_input(image, size);
        progress(0.1);
        let shape = [1usize, 3, size as usize, size as usize];
        let mut session = self
            .session
            .lock()
            .map_err(|_| err("the model session was poisoned by an earlier panic"))?;
        let outputs = if self.half_input {
            let halves: Vec<half::f16> = data.iter().map(|v| half::f16::from_f32(*v)).collect();
            let tensor = Tensor::from_array((shape, halves)).map_err(err)?;
            session.run(ort::inputs![tensor]).map_err(err)?
        } else {
            let tensor = Tensor::from_array((shape, data)).map_err(err)?;
            session.run(ort::inputs![tensor]).map_err(err)?
        };
        progress(0.8);
        let (_, first) = outputs
            .iter()
            .next()
            .ok_or_else(|| err("the model returned no outputs"))?;
        let (dims, values): (Vec<i64>, Vec<f32>) = match first.try_extract_tensor::<f32>() {
            Ok((shape, v)) => (shape.to_vec(), v.to_vec()),
            Err(_) => {
                let (shape, v) = first.try_extract_tensor::<half::f16>().map_err(err)?;
                (shape.to_vec(), v.iter().map(|h| h.to_f32()).collect())
            }
        };
        let output_size = *dims
            .last()
            .ok_or_else(|| err("the model output has no shape"))? as u32;
        let plane = (output_size * output_size) as usize;
        if values.len() < plane {
            return Err(err(format!(
                "the model output has {} values, fewer than {output_size}^2",
                values.len()
            )));
        }
        // Some exports end in a sigmoid already. logits_to_mask applies one, so
        // turn probabilities back into logits rather than squash them twice.
        let plane_values = &values[values.len() - plane..];
        let (lo, hi) = plane_values
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let logits: Vec<f32> = if lo >= 0.0 && hi <= 1.0 {
            plane_values
                .iter()
                .map(|p| {
                    let p = p.clamp(1e-6, 1.0 - 1e-6);
                    (p / (1.0 - p)).ln()
                })
                .collect()
        } else {
            plane_values.to_vec()
        };
        let model_scale = scale * output_size as f32 / size as f32;
        let ox = (offset_x as f32 * output_size as f32 / size as f32).round() as u32;
        let oy = (offset_y as f32 * output_size as f32 / size as f32).round() as u32;
        let mask = logits_to_mask(&logits, width, height, output_size, model_scale, ox, oy)?;
        progress(1.0);
        Ok(mask)
    }
}
