//! Errors a user can act on.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KataError {
    #[error("could not decode the image: {0}")]
    Image(#[from] image::ImageError),

    #[error("file error: {0}")]
    Io(#[from] std::io::Error),

    #[error("the image is {width}x{height}, larger than the {limit} pixel limit")]
    TooLarge { width: u32, height: u32, limit: u64 },

    #[error("nothing was found to cut out: the mask came back empty")]
    EmptySubject,

    #[error("the segmentation model failed: {0}")]
    Segmentation(String),

    #[error("model error: {0}")]
    Model(String),

    #[error("invalid argument: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, KataError>;
