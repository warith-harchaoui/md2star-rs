//! Error type for the crate.
//!
//! The library never panics on bad input or a failed write and never prints — it returns
//! a typed [`Error`] so the binary (or a future WASM/host caller) decides how to surface
//! it. Two failure modes exist today: reading the input file and packing the `.docx` zip.

use thiserror::Error;

/// Everything that can go wrong turning Markdown into a `.docx` on disk.
#[derive(Debug, Error)]
pub enum Error {
    /// The input file could not be read, or the output file could not be created.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// `docx-rs` failed to serialize/zip the document. Its own error types vary across
    /// the build/pack calls, so we flatten them to a message at the boundary.
    #[error("failed to write DOCX: {0}")]
    Docx(String),
}

/// Crate-wide result alias so signatures read `Result<T>` rather than the full path.
pub type Result<T> = std::result::Result<T, Error>;
