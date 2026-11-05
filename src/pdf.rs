//! PDF output, in process — no LibreOffice, no LaTeX, no subprocess (feature `pdf`).
//!
//! # What this module must do
//!
//! Render a [`Document`] to PDF inside the process, in pure Rust, so it works on iOS
//! and Android. The lead to evaluate first: `typst` as a library (Markdown AST → Typst
//! markup → PDF, with its own math, fonts embedded) or `krilla` (a lower-level PDF
//! writer we lay out ourselves). Choose on binary size, compile time and whether
//! `cargo check --target aarch64-apple-ios` / `aarch64-linux-android` still passes with
//! `--features pdf`; say which in the README. Fonts must be embedded in the binary or
//! supplied by the caller: a phone has no system font directory to rely on. Output
//! must be byte-identical run to run (no creation date, or a fixed one).
//!
//! The Python md2star makes its PDF by converting its DOCX with LibreOffice; matching
//! that page for page is not the goal — a faithful, well-set PDF of the same document is.
//!
//! Owned by the twin + PDF port (see [`crate::twin`]).

use crate::ast::Document;
use crate::error::{Error, Result};
use crate::options::{Options, Rendered};

/// Render `doc` to PDF bytes.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — the PDF writer is not ported yet.
pub fn render(doc: &Document, options: &Options) -> Result<Rendered> {
    let _ = (doc, options);
    Err(Error::unsupported("PDF output"))
}
