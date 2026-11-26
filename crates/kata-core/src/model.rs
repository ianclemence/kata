//! The model registry and its checksums.
//!
//! No model ships with Kata. Each one is fetched on first use, resumable, and
//! verified against the SHA-256 that the publisher records for the file. The
//! values below were read from the publisher's own LFS metadata, not typed from
//! memory: they are the identity of the bytes the app will accept.

use crate::error::{KataError, Result};
use serde::{Deserialize, Serialize};

/// One downloadable model.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// File name on disk once downloaded.
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub license: &'static str,
    pub license_url: &'static str,
    /// How big the matting input is.
    pub input_size: u32,
    /// One line for the settings screen.
    pub note: &'static str,
}

impl ModelSpec {
    pub fn megabytes(&self) -> f32 {
        self.bytes as f32 / (1024.0 * 1024.0)
    }
}

/// Every model Kata will download. All are MIT licensed.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "birefnet-lite-fp16",
        name: "BiRefNet lite (fp16)",
        file: "birefnet-lite-fp16.onnx",
        url: "https://huggingface.co/onnx-community/BiRefNet_lite-ONNX/resolve/main/onnx/model_fp16.onnx",
        sha256: "d39b897ceb16ae654c1731f3dba0cf9b368d9cae74b5a57459b455cc8bfec402",
        bytes: 114_538_221,
        license: "MIT",
        license_url: "https://huggingface.co/onnx-community/BiRefNet_lite-ONNX",
        input_size: 1024,
        note: "The default in the browser and the fast option on desktop.",
    },
    ModelSpec {
        id: "birefnet-lite-fp32",
        name: "BiRefNet lite (fp32)",
        file: "birefnet-lite-fp32.onnx",
        url: "https://huggingface.co/onnx-community/BiRefNet_lite-ONNX/resolve/main/onnx/model.onnx",
        sha256: "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333",
        bytes: 224_005_088,
        license: "MIT",
        license_url: "https://huggingface.co/onnx-community/BiRefNet_lite-ONNX",
        input_size: 1024,
        note: "The fast option on a CPU-only machine, where fp16 may be unsupported.",
    },
    ModelSpec {
        id: "birefnet-general-fp16",
        name: "BiRefNet general (fp16)",
        file: "birefnet-general-fp16.onnx",
        url: "https://huggingface.co/onnx-community/BiRefNet-ONNX/resolve/main/onnx/model_fp16.onnx",
        sha256: "3654c741eb80bd926ada8fed1713b506ccf8d30eb1f6487e87eb9f234f33df09",
        bytes: 489_666_272,
        license: "MIT",
        license_url: "https://huggingface.co/onnx-community/BiRefNet-ONNX",
        input_size: 1024,
        note: "The quality option: the best edges, and the slowest.",
    },
];

/// The desktop default when a GPU execution provider is available.
pub const DEFAULT_GPU_MODEL: &str = "birefnet-general-fp16";
/// The desktop default on the CPU, and the web default.
pub const DEFAULT_CPU_MODEL: &str = "birefnet-lite-fp32";
/// The web default: small enough to fetch on a phone connection.
pub const DEFAULT_WEB_MODEL: &str = "birefnet-lite-fp16";
/// The optional higher-quality download on the web.
pub const WEB_QUALITY_MODEL: &str = "birefnet-general-fp16";

pub fn by_id(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn require(id: &str) -> Result<&'static ModelSpec> {
    by_id(id).ok_or_else(|| KataError::Model(format!("there is no model called `{id}`")))
}

/// The model to use when the person has not chosen one.
pub fn default_for(gpu: bool) -> &'static ModelSpec {
    let id = if gpu {
        DEFAULT_GPU_MODEL
    } else {
        DEFAULT_CPU_MODEL
    };
    by_id(id).expect("the default model ids are in the registry")
}

/// Hex SHA-256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Check bytes against a spec's published checksum.
pub fn verify(bytes: &[u8], spec: &ModelSpec) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(spec.sha256) {
        return Ok(());
    }
    Err(KataError::Model(format!(
        "{} did not match its checksum.\n  expected {}\n  got      {}\n\
         The file was deleted; try the download again.",
        spec.file, spec.sha256, actual
    )))
}

/// Whether a downloaded file is the right size. A cheap gate before hashing,
/// so a truncated download is rejected without reading half a gigabyte.
pub fn check_size(bytes: u64, spec: &ModelSpec) -> Result<()> {
    if bytes == spec.bytes {
        Ok(())
    } else {
        Err(KataError::Model(format!(
            "{} is {bytes} bytes, expected {}; the download may be incomplete",
            spec.file, spec.bytes
        )))
    }
}

/// A downloaded, verified model.
pub struct ModelFile {
    pub spec: &'static ModelSpec,
    pub bytes: Vec<u8>,
}

/// Where models live. On desktop this is the OS app-data folder; the browser
/// keeps them in the Cache API or OPFS and never calls this.
#[cfg(not(target_arch = "wasm32"))]
pub fn model_dir() -> std::path::PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
    };
    base.unwrap_or_else(std::env::temp_dir).join("kata/models")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn model_path(spec: &ModelSpec) -> std::path::PathBuf {
    model_dir().join(spec.file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_self_consistent() {
        for spec in MODELS {
            assert!(!spec.sha256.is_empty(), "{} has no checksum", spec.id);
            assert_eq!(spec.sha256.len(), 64, "{} has a bad checksum", spec.id);
            assert!(
                spec.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} checksum is not hex",
                spec.id
            );
            assert!(spec.bytes > 0);
            assert!(spec.url.starts_with("https://"), "{} is not https", spec.id);
            assert!(by_id(spec.id).is_some());
        }
    }

    #[test]
    fn no_model_id_or_file_is_duplicated() {
        for (i, a) in MODELS.iter().enumerate() {
            for b in &MODELS[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.file, b.file);
                assert_ne!(a.url, b.url);
            }
        }
    }

    #[test]
    fn only_permissive_licences_are_shipped() {
        for spec in MODELS {
            assert!(
                matches!(spec.license, "MIT" | "Apache-2.0" | "BSD-3-Clause"),
                "{} is {} which is not acceptable for shipping",
                spec.id,
                spec.license
            );
        }
    }

    #[test]
    fn the_web_default_fits_the_size_budget() {
        let spec = by_id(DEFAULT_WEB_MODEL).unwrap();
        assert!(
            spec.megabytes() <= 180.0,
            "the web default is {} MB",
            spec.megabytes()
        );
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        // The empty string, from the SHA-256 specification.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_rejects_the_wrong_bytes() {
        let spec = by_id("birefnet-lite-fp16").unwrap();
        assert!(verify(b"not a model", spec).is_err());
        assert!(check_size(1234, spec).is_err());
        assert!(check_size(spec.bytes, spec).is_ok());
    }

    #[test]
    fn an_unknown_model_is_a_clear_error() {
        assert!(require("nope").is_err());
    }

    #[test]
    fn defaults_resolve() {
        assert_eq!(default_for(true).id, DEFAULT_GPU_MODEL);
        assert_eq!(default_for(false).id, DEFAULT_CPU_MODEL);
    }
}
