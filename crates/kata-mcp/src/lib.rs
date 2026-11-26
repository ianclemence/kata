//! Kata as an MCP server (Model Context Protocol, stdio transport).
//!
//! An agent gets the whole workflow as tools: segment with hints, edit masks,
//! print in brand inks, compose a poster from a scene, review it, look closely
//! at any region, and export to platform sizes. The design judgement lives in
//! the bundled skills, served as resources, so the same tools serve any brand.
//!
//! Every output is a file in the work folder with a short id ("art_3_cutout"),
//! so a later call can say `"image": "art_3_cutout"` instead of a long path.
//! Pictures come back as small previews; full-size files stay on disk.

#![allow(clippy::needless_range_loop)]

mod protocol;
mod skills;
mod tools;

pub use protocol::{handle, serve};
pub use tools::{definitions, Server};

use std::path::PathBuf;

/// How the server is allowed to touch the disk.
#[derive(Clone, Debug)]
pub struct Config {
    /// Where outputs go. Created if missing.
    pub work_dir: PathBuf,
    /// Folders inputs may be read from and outputs written to. The work
    /// folder is always allowed.
    pub roots: Vec<PathBuf>,
}

impl Config {
    pub fn new(work_dir: PathBuf, roots: Vec<PathBuf>) -> Config {
        Config { work_dir, roots }
    }
}
