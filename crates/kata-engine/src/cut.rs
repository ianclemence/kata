//! Segmentation as a service to the rest of Kata: hints, a loaded-model pool
//! and a mask cache keyed by the image's bytes.
//!
//! BiRefNet takes the better part of a minute on a laptop CPU. An agent
//! iterating on a poster asks for the same cutout many times, so the second ask
//! must be instant: masks are cached on disk by (image, model, hint).

use crate::models::{self, Resolved};
use crate::onnx::OnnxSegmenter;
use crate::runtime;
use image::{GrayImage, RgbaImage};
use kata_core::error::{KataError, Result};
use kata_core::mask::Mask;
use kata_core::model::sha256_hex;
use kata_core::ops::{self, Hint, MaskOp};
use kata_core::segment::{LumaSegmenter, Segmenter};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// How to segment.
#[derive(Clone, Debug, Default)]
pub struct CutOptions {
    /// Registry id or path; `None` resolves the default.
    pub model: Option<String>,
    pub hint: Hint,
    /// Skip the cache (both read and write).
    pub fresh: bool,
    /// Use the lightness split instead of a model. Tests and demos only.
    pub luma: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CutReport {
    pub model: String,
    pub cached: bool,
    pub seconds: f32,
    /// The region the model saw: x, y, w, h.
    pub region: [u32; 4],
    /// Share of the image that is subject, 0..1.
    pub coverage: f32,
    pub cache_key: String,
}

type Pool = Mutex<HashMap<PathBuf, Arc<OnnxSegmenter>>>;
static POOL: OnceLock<Pool> = OnceLock::new();

/// Load a model once per process and share it.
pub fn segmenter(resolved: &Resolved) -> Result<Arc<OnnxSegmenter>> {
    let pool = POOL.get_or_init(|| Mutex::new(HashMap::new()));
    let mut pool = pool
        .lock()
        .map_err(|_| KataError::Segmentation("model pool poisoned".into()))?;
    if let Some(found) = pool.get(&resolved.path) {
        return Ok(found.clone());
    }
    let loaded = Arc::new(OnnxSegmenter::load(
        &resolved.path,
        &resolved.id,
        &resolved.name,
    )?);
    pool.insert(resolved.path.clone(), loaded.clone());
    Ok(loaded)
}

pub fn cache_dir() -> PathBuf {
    runtime::data_dir().join("cache").join("masks")
}

fn cache_key(image_hash: &str, model_id: &str, hint: &Hint) -> String {
    let hint = serde_json::to_string(hint).unwrap_or_default();
    sha256_hex(format!("{image_hash}|{model_id}|{hint}").as_bytes())
}

fn read_cached(key: &str, width: u32, height: u32) -> Option<Mask> {
    let path = cache_dir().join(format!("{key}.png"));
    let gray = image::open(&path).ok()?.to_luma8();
    (gray.dimensions() == (width, height)).then(|| Mask {
        width,
        height,
        data: gray.into_raw(),
    })
}

fn write_cached(key: &str, mask: &Mask) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Some(gray) = GrayImage::from_raw(mask.width, mask.height, mask.data.clone()) {
            let _ = gray.save(dir.join(format!("{key}.png")));
        }
    }
}

/// Segment `image`. `image_hash` identifies the source bytes (sha256 of the file);
/// pass the same hash for the same photo and the cache does the rest.
pub fn segment(
    image: &RgbaImage,
    image_hash: &str,
    options: &CutOptions,
) -> Result<(Mask, CutReport)> {
    let started = Instant::now();
    let (width, height) = image.dimensions();
    let (x, y, w, h) = ops::hint_region(&options.hint, width, height)?;

    // Resolve the model (cheap) but load it (slow) only on a cache miss.
    let resolved = if options.luma {
        None
    } else {
        Some(models::resolve(options.model.as_deref())?)
    };
    let model_id = resolved
        .as_ref()
        .map(|r| r.id.clone())
        .unwrap_or_else(|| "luma".into());

    let key = cache_key(image_hash, &model_id, &options.hint);
    if !options.fresh {
        if let Some(mask) = read_cached(&key, width, height) {
            let coverage = mask.coverage();
            return Ok((
                mask,
                CutReport {
                    model: model_id,
                    cached: true,
                    seconds: started.elapsed().as_secs_f32(),
                    region: [x, y, w, h],
                    coverage,
                    cache_key: key,
                },
            ));
        }
    }

    let crop = if (x, y, w, h) == (0, 0, width, height) {
        image.clone()
    } else {
        image::imageops::crop_imm(image, x, y, w, h).to_image()
    };
    let crop_mask = match &resolved {
        Some(r) => segmenter(r)?.segment(&crop, &mut |_| {})?,
        None => LumaSegmenter::default().segment(&crop, &mut |_| {})?,
    };
    let mut mask = ops::place(&crop_mask, x, y, width, height);
    if let Hint::Points { points } = &options.hint {
        mask = ops::apply(
            &mask,
            None,
            &[MaskOp::KeepAt {
                points: points.clone(),
            }],
        )?;
    }
    if !options.fresh {
        write_cached(&key, &mask);
    }
    let coverage = mask.coverage();
    if coverage == 0.0 {
        return Err(KataError::EmptySubject);
    }
    Ok((
        mask,
        CutReport {
            model: model_id,
            cached: false,
            seconds: started.elapsed().as_secs_f32(),
            region: [x, y, w, h],
            coverage,
            cache_key: key,
        },
    ))
}

/// The cutout: the photo with the mask as its alpha.
pub fn cutout(image: &RgbaImage, mask: &Mask) -> RgbaImage {
    let mut out = image.clone();
    for (p, a) in out.pixels_mut().zip(&mask.data) {
        p.0[3] = ((u16::from(p.0[3]) * u16::from(*a)) / 255) as u8;
    }
    out
}

/// Crop transparent margins away, keeping `pad` pixels.
pub fn trim(image: &RgbaImage, pad: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if p.0[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 {
        return image.clone();
    }
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    image::imageops::crop_imm(image, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(80, 60, image::Rgba([235, 235, 235, 255]));
        for y in 10..50 {
            for x in 20..40 {
                img.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
            }
            for x in 55..70 {
                img.put_pixel(x, y, image::Rgba([25, 25, 25, 255]));
            }
        }
        img
    }

    #[test]
    fn the_second_cut_comes_from_the_cache() {
        let home = std::env::temp_dir().join(format!("kata-cut-test-{}", std::process::id()));
        std::env::set_var("KATA_HOME", &home);
        let options = CutOptions {
            luma: true,
            ..CutOptions::default()
        };
        let (first, report) = segment(&picture(), "abc", &options).unwrap();
        assert!(!report.cached);
        let (second, report) = segment(&picture(), "abc", &options).unwrap();
        assert!(report.cached);
        assert_eq!(first, second);
        std::env::remove_var("KATA_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_point_hint_keeps_one_of_two_subjects() {
        let options = CutOptions {
            luma: true,
            fresh: true,
            hint: Hint::Points {
                points: vec![[60, 30]],
            },
            ..CutOptions::default()
        };
        let (mask, _) = segment(&picture(), "two", &options).unwrap();
        assert!(mask.get(60, 30) > 128);
        assert_eq!(mask.get(30, 30), 0);
    }

    #[test]
    fn a_box_hint_only_looks_inside_the_box() {
        let options = CutOptions {
            luma: true,
            fresh: true,
            hint: Hint::Box {
                x: 50,
                y: 5,
                w: 25,
                h: 50,
                margin: 0.0,
            },
            ..CutOptions::default()
        };
        let (mask, report) = segment(&picture(), "box", &options).unwrap();
        assert_eq!(report.region, [50, 5, 25, 50]);
        assert_eq!(mask.get(30, 30), 0);
    }

    #[test]
    fn trim_removes_empty_margins() {
        let mut img = RgbaImage::new(50, 50);
        img.put_pixel(20, 30, image::Rgba([1, 2, 3, 255]));
        assert_eq!(trim(&img, 2).dimensions(), (5, 5));
    }
}
