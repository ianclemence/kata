//! Getting a model onto disk: resumable download, size and SHA-256 checks.

use kata_core::error::{KataError, Result};
use kata_core::model::{self, ModelSpec};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
pub struct ModelStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub megabytes: f32,
    pub license: &'static str,
    pub note: &'static str,
    pub path: PathBuf,
    pub downloaded: bool,
}

pub fn status() -> Vec<ModelStatus> {
    model::MODELS
        .iter()
        .map(|spec| {
            let path = model::model_path(spec);
            ModelStatus {
                id: spec.id,
                name: spec.name,
                megabytes: spec.megabytes(),
                license: spec.license,
                note: spec.note,
                downloaded: path.is_file(),
                path,
            }
        })
        .collect()
}

/// A model to run: a file on disk and how to name it in reports.
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// The model to use. `choice` is a registry id, a path to a `.onnx` file, or
/// nothing (then `KATA_MODEL`, then the first downloaded registry model, then
/// the CPU default).
pub fn resolve(choice: Option<&str>) -> Result<Resolved> {
    let env_choice = std::env::var("KATA_MODEL").ok();
    let choice = choice.map(str::to_owned).or(env_choice);
    if let Some(choice) = choice {
        let as_path = PathBuf::from(&choice);
        if as_path.is_file() {
            let stem = as_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("custom")
                .to_owned();
            return Ok(Resolved {
                id: format!("file:{stem}"),
                name: stem,
                path: as_path,
            });
        }
        let spec = model::require(&choice)?;
        let path = model::model_path(spec);
        if !path.is_file() {
            return Err(KataError::Model(format!(
                "{} is not downloaded yet. Run `kata models download {}` ({:.0} MB, {}).",
                spec.id,
                spec.id,
                spec.megabytes(),
                spec.license
            )));
        }
        return Ok(Resolved {
            id: spec.id.into(),
            name: spec.name.into(),
            path,
        });
    }
    if let Some(found) = status().into_iter().find(|s| s.downloaded) {
        return Ok(Resolved {
            id: found.id.into(),
            name: found.name.into(),
            path: found.path,
        });
    }
    let spec = model::default_for(false);
    Err(KataError::Model(format!(
        "no segmentation model is downloaded. Run `kata models download {}` ({:.0} MB), \
         or pass a path to any BiRefNet .onnx file with --model.",
        spec.id,
        spec.megabytes()
    )))
}

/// Download a registry model, resuming a partial file, then verify it.
/// `progress(done, total)` is called as bytes arrive.
pub fn download(spec: &ModelSpec, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf> {
    let path = model::model_path(spec);
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().expect("model path has a folder"))?;
    let part = path.with_extension("onnx.part");
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut request = ureq::get(spec.url).header("User-Agent", "kata (model download)");
    if have > 0 && have < spec.bytes {
        request = request.header("Range", &format!("bytes={have}-"));
    }
    let response = request
        .call()
        .map_err(|e| KataError::Model(format!("could not download {}: {e}", spec.id)))?;
    let resumed = response.status().as_u16() == 206;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)?;
    let mut done = if resumed { have } else { 0 };
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(spec.bytes + 1024)
        .reader();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| {
            KataError::Model(format!(
                "the download of {} stopped: {e}. Run the command again to resume.",
                spec.id
            ))
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        done += n as u64;
        progress(done, spec.bytes);
    }
    file.flush()?;
    drop(file);
    model::check_size(std::fs::metadata(&part)?.len(), spec)?;
    let bytes = std::fs::read(&part)?;
    if let Err(e) = model::verify(&bytes, spec) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_to_any_file_is_accepted_as_a_custom_model() {
        let dir = std::env::temp_dir().join("kata-models-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("my-matting.onnx");
        std::fs::write(&file, b"not really a model").unwrap();
        let resolved = resolve(Some(file.to_str().unwrap())).unwrap();
        assert_eq!(resolved.id, "file:my-matting");
        assert_eq!(resolved.path, file);
    }

    #[test]
    fn an_unknown_id_is_a_clear_error() {
        let error = resolve(Some("no-such-model")).unwrap_err().to_string();
        assert!(error.contains("no-such-model"), "{error}");
    }
}
