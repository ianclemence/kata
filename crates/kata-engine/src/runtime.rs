//! Finding ONNX Runtime and the folders Kata keeps its data in.
//!
//! Kata links ONNX Runtime at run time (ort's `load-dynamic`), so one binary
//! works with whichever runtime is present and the release can ship the library
//! next to the executable. Search order is predictable and reported: an agent
//! asking "why is there no model?" gets the exact paths that were tried.

use kata_core::error::{KataError, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// File name of the runtime library on this platform.
pub fn library_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

/// Kata's data folder: models, the runtime, the mask cache, MCP artifacts.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("KATA_HOME") {
        return PathBuf::from(dir);
    }
    kata_core::model::model_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("kata"))
}

/// Every place Kata looks for the runtime, in order.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["KATA_ORT_DYLIB", "ORT_DYLIB_PATH"] {
        if let Some(path) = std::env::var_os(var) {
            out.push(PathBuf::from(path));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join(library_name()));
            out.push(dir.join("lib").join(library_name()));
            // macOS app bundles and Linux installs keep libraries beside bin/.
            if let Some(up) = dir.parent() {
                out.push(up.join("lib").join(library_name()));
            }
        }
    }
    out.push(data_dir().join("runtime").join(library_name()));
    out
}

pub fn find_library() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file())
}

static ENVIRONMENT: OnceLock<std::result::Result<ort::environment::Environment, String>> =
    OnceLock::new();

/// Load ONNX Runtime once per process.
pub fn environment() -> Result<ort::environment::Environment> {
    ENVIRONMENT
        .get_or_init(|| {
            let Some(path) = find_library() else {
                let tried = candidates()
                    .iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(format!(
                    "ONNX Runtime ({}) was not found. Looked in:\n{tried}\n\
                     Put the library in one of those places, or set KATA_ORT_DYLIB to its path. \
                     Release builds ship it next to the kata binary.",
                    library_name()
                ));
            };
            ort::init_from(&path)
                .map_err(|e| format!("could not load ONNX Runtime from {}: {e}", path.display()))?
                .with_name("kata")
                .build()
                .map_err(|e| format!("ONNX Runtime at {} failed to start: {e}", path.display()))
        })
        .clone()
        .map_err(KataError::Segmentation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_variable_comes_first() {
        std::env::set_var("KATA_ORT_DYLIB", "/somewhere/libonnxruntime.so");
        assert_eq!(
            candidates()[0],
            PathBuf::from("/somewhere/libonnxruntime.so")
        );
        std::env::remove_var("KATA_ORT_DYLIB");
    }

    #[test]
    fn kata_home_overrides_the_data_folder() {
        std::env::set_var("KATA_HOME", "/tmp/kata-test-home");
        assert_eq!(data_dir(), PathBuf::from("/tmp/kata-test-home"));
        std::env::remove_var("KATA_HOME");
    }
}
