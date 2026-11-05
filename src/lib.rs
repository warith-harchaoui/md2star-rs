//! `md2star-rs` — Markdown → DOCX / PPTX / PDF and back, in pure Rust: no Pandoc, no
//! LibreOffice, no subprocess, no network unless asked.
//!
//! The Python `md2star` wraps Pandoc. This crate reaches the same goal in process, so the
//! core compiles for iOS and Android (`cargo check --lib --no-default-features --target
//! aarch64-apple-ios`) as well as to a single static binary:
//!
//! ```text
//!                                                   ┌──▶ docx  ──▶ .docx
//! Markdown ──▶ reader ──▶ ast::Document ──(bib)─────┼──▶ pptx  ──▶ .pptx
//!                │                                  └──▶ pdf   ──▶ .pdf   (feature `pdf`)
//!                └── resources (images: path, data URI, URL with feature `net`)
//!
//! .docx / .pptx ──▶ twin ──▶ Markdown          .pptx ──▶ audit ──▶ findings
//! ```
//!
//! # Module map and ownership
//!
//! | Module | Role | Owner |
//! |---|---|---|
//! | [`ast`], [`reader`], [`resources`], [`options`], [`error`], `cli/mod.rs` | the shared foundation | phase 1 (frozen) |
//! | [`docx`] (+ `math`, `citations`, `images`), [`bib`], `cli/docx.rs` | Word output, LaTeX → OMML, BibTeX | DOCX port |
//! | [`pptx`] (+ `layout`, `template`, `images`), [`audit`], [`templates`], `llm`, `cli/{pptx,audit,templates}.rs` | PowerPoint output, deck audit | PPTX port |
//! | [`twin`] (+ `docx`, `pptx`), `pdf`, `cli/{twin,pdf}.rs` | Office → Markdown, PDF output | twin + PDF port |
//!
//! Every function that is not ported yet returns [`Error::Unsupported`], never panics.
//!
//! # Cargo features
//!
//! - `cli` (default): the `md2star-rs`, `md2docx` and `md2pptx` binaries.
//! - `pdf`: in-process PDF output.
//! - `net`: fetch `http(s)://` images (still opt-in per conversion).
//! - `llm`: model-assisted layout selection through an OpenAI-compatible endpoint.
//!
//! # Examples
//!
//! ```
//! use md2star_rs::{convert, Format, Options};
//!
//! let out = convert("# Hi\n\nHello.", Format::Docx, &Options::default()).unwrap();
//! assert_eq!(&out.bytes[..2], b"PK"); // a .docx is a zip
//! assert!(out.warnings.is_empty());
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fs;
use std::path::Path;

pub mod ast;
pub mod audit;
pub mod bib;
pub mod docx;
pub mod error;
pub mod options;
pub mod pptx;
pub mod reader;
pub mod resources;
pub mod templates;
pub mod twin;

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "llm")]
pub mod llm;
#[cfg(feature = "pdf")]
pub mod pdf;

pub use ast::Document;
pub use error::{Error, Result};
pub use options::{Format, Options, Rendered};

/// The DOCX writer's v0.4 name, kept so existing callers still compile.
#[deprecated(since = "0.5.0", note = "the DOCX writer moved to `md2star_rs::docx`")]
pub mod writer {
    pub use crate::docx::{build, build_with_reference};
}

/// Convert Markdown to `format`, in memory.
///
/// # Errors
///
/// Whatever the backend reports; [`Error::Unsupported`] for a backend that is not
/// ported yet or not compiled in (PDF without the `pdf` feature).
pub fn convert(markdown: &str, format: Format, options: &Options) -> Result<Rendered> {
    render(&reader::read(markdown), format, options)
}

/// Render an already-parsed document to `format`.
///
/// # Errors
///
/// See [`convert`].
pub fn render(doc: &Document, format: Format, options: &Options) -> Result<Rendered> {
    match format {
        Format::Docx => docx::render(doc, options),
        Format::Pptx => pptx::render(doc, options),
        #[cfg(feature = "pdf")]
        Format::Pdf => pdf::render(doc, options),
        #[cfg(not(feature = "pdf"))]
        Format::Pdf => Err(Error::unsupported(
            "PDF output: this build of md2star-rs was compiled without the `pdf` feature",
        )),
    }
}

/// Convert a Markdown string to `.docx` bytes in memory.
///
/// # Examples
///
/// ```
/// let bytes = md2star_rs::markdown_to_docx_bytes("Hello **world**").unwrap();
/// // A .docx is a zip, so it starts with the local-file-header magic `PK\x03\x04`.
/// assert_eq!(&bytes[..2], b"PK");
/// ```
pub fn markdown_to_docx_bytes(markdown: &str) -> Result<Vec<u8>> {
    docx::pack(docx::build(&reader::parse(markdown)))
}

/// Convert Markdown to `.docx` bytes, inheriting styling from a reference `template` `.docx`.
///
/// The template's styles, theme, fonts and page setup are kept, its body content is
/// dropped, and headings / block quotes / code / list items are emitted through the
/// template's named styles when present.
///
/// # Errors
///
/// Returns [`Error::Template`] if `template_docx` is not a readable `.docx`, or [`Error::Docx`]
/// if packing the result fails.
pub fn markdown_to_docx_bytes_with_reference(
    markdown: &str,
    template_docx: &[u8],
) -> Result<Vec<u8>> {
    let template = docx_rs::read_docx(template_docx).map_err(|e| Error::Template(e.to_string()))?;
    docx::pack(docx::build_with_reference(
        &reader::parse(markdown),
        template,
    ))
}

/// Convert a Markdown string and write the resulting `.docx` to `output`.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// md2star_rs::markdown_to_docx_file("# Title", Path::new("title.docx")).unwrap();
/// ```
pub fn markdown_to_docx_file(markdown: &str, output: &Path) -> Result<()> {
    let bytes = markdown_to_docx_bytes(markdown)?;
    fs::write(output, bytes)?;
    Ok(())
}

/// Read a Markdown file and write the converted `.docx` to `output`.
pub fn convert_path(input: &Path, output: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    markdown_to_docx_file(&markdown, output)
}

/// Read a Markdown file and write the converted `.docx` to `output`, styled after `reference`.
///
/// # Errors
///
/// Returns [`Error::Io`] if `input` or `reference` cannot be read or `output` cannot be written,
/// [`Error::Template`] if `reference` is not a valid `.docx`, or [`Error::Docx`] on a pack failure.
pub fn convert_path_with_reference(input: &Path, output: &Path, reference: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    let template = fs::read(reference)?;
    let bytes = markdown_to_docx_bytes_with_reference(&markdown, &template)?;
    fs::write(output, bytes)?;
    Ok(())
}

/// Convert a Markdown string to `.pptx` bytes in memory.
///
/// # Errors
///
/// Returns [`Error::Pptx`] if the presentation cannot be assembled or packed.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "legacy-pptx")] {
/// let bytes = md2star_rs::markdown_to_pptx_bytes("# Title\n\n- one\n- two").unwrap();
/// assert_eq!(&bytes[..2], b"PK");
/// # }
/// ```
pub fn markdown_to_pptx_bytes(markdown: &str) -> Result<Vec<u8>> {
    pptx::build(&reader::parse(markdown))
}

/// Convert a Markdown string and write the resulting `.pptx` to `output`.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// md2star_rs::markdown_to_pptx_file("# Slide", Path::new("deck.pptx")).unwrap();
/// ```
pub fn markdown_to_pptx_file(markdown: &str, output: &Path) -> Result<()> {
    let bytes = markdown_to_pptx_bytes(markdown)?;
    fs::write(output, bytes)?;
    Ok(())
}

/// Read a Markdown file and write the converted `.pptx` to `output`.
pub fn convert_path_to_pptx(input: &Path, output: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    markdown_to_pptx_file(&markdown, output)
}
