//! `md2star-rs` — a pure-Rust Markdown → DOCX writer, no Pandoc, no subprocess.
//!
//! Module summary
//! --------------
//! The Python `md2star` is a thin wrapper around Pandoc. This crate keeps the same goal —
//! Markdown in, a faithful `.docx` out — but reaches it entirely in Rust, so it compiles
//! to a single static binary that runs on any OS/device with no Pandoc install:
//!
//! ```text
//! Markdown ──▶ reader (pulldown-cmark → AST) ──▶ writer (AST → docx-rs) ──▶ .docx
//! ```
//!
//! The named AST in the middle ([`ast`]) is the reader/writer seam Pandoc uses internally;
//! it is what will let a second backend (Typst, HTML) slot in later without touching the
//! reader. See `README.md` for the exact scope and the (deliberate) v0.1 limitations
//! versus the Pandoc-backed original.
//!
//! # Examples
//!
//! ```no_run
//! use std::path::Path;
//! // Convert a Markdown string straight to a .docx on disk.
//! md2star_rs::markdown_to_docx_file("# Hi\n\nHello.", Path::new("out.docx")).unwrap();
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fs;
use std::io::Cursor;
use std::path::Path;

pub mod ast;
pub mod error;
pub mod reader;
pub mod writer;

pub use error::{Error, Result};

/// Convert a Markdown string to `.docx` bytes in memory.
///
/// Useful for tests, servers, and a future WASM entry point where writing to disk isn't
/// wanted. The bytes are a complete, valid OOXML zip.
///
/// # Examples
///
/// ```
/// let bytes = md2star_rs::markdown_to_docx_bytes("Hello **world**").unwrap();
/// // A .docx is a zip, so it starts with the local-file-header magic `PK\x03\x04`.
/// assert_eq!(&bytes[..2], b"PK");
/// ```
pub fn markdown_to_docx_bytes(markdown: &str) -> Result<Vec<u8>> {
    let blocks = reader::parse(markdown);
    let docx = writer::build(&blocks);

    // `docx-rs` packs into any `Write + Seek`; an in-memory cursor gives us the raw bytes.
    let mut buffer = Cursor::new(Vec::new());
    docx.build()
        .pack(&mut buffer)
        .map_err(|e| Error::Docx(e.to_string()))?;
    Ok(buffer.into_inner())
}

/// Convert a Markdown string and write the resulting `.docx` to `output`.
///
/// Parent directories are assumed to exist; only the file itself is created/truncated.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// md2star_rs::markdown_to_docx_file("# Title", Path::new("title.docx")).unwrap();
/// ```
pub fn markdown_to_docx_file(markdown: &str, output: &Path) -> Result<()> {
    // Reuse the in-memory path, then commit the bytes in one write — this keeps the
    // "build" and "persist" concerns separate and makes partial-file states unlikely.
    let bytes = markdown_to_docx_bytes(markdown)?;
    fs::write(output, bytes)?;
    Ok(())
}

/// Read a Markdown file and write the converted `.docx` to `output`.
///
/// The convenience the CLI is built on: it wires the filesystem read to
/// [`markdown_to_docx_file`] so the binary stays a thin argument-parsing shell.
pub fn convert_path(input: &Path, output: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    markdown_to_docx_file(&markdown, output)
}
