//! `twin`: read a finished document back into an editable Markdown twin.
//!
//! # What this module must do
//!
//! Port `md2star twin` (`md2star/reverse.py`) for Office documents, as pure XML over the
//! zip — portable, no Office, no Python:
//!
//! - **DOCX** ([`docx`]): headings (by style and outline level), paragraphs with
//!   bold/italic/code runs, lists (numbering → `-` / `1.`), tables (→ pipe tables), real
//!   hyperlinks, footnotes (→ `[^n]`), OMML equations back to LaTeX where the subset
//!   allows it, images extracted to a media folder and referenced with their alt text.
//! - **PPTX** ([`pptx`]): one `#`/`##` slide per slide, title from the title
//!   placeholder, body bullets with their levels, tables, pictures, speaker notes as
//!   `::: notes`, `---` between slides.
//!
//! **PDF → Markdown is out of scope** without OCR and a layout model: [`to_markdown`]
//! refuses it with an [`Error::Unsupported`] that says so. Re-authoring figures as
//! Mermaid/SVG (`--diagrams`) needs a vision model and is not part of the deterministic
//! port.
//!
//! # Files this module owns
//!
//! `src/twin/` (this file, `docx.rs`, `pptx.rs`), `src/pdf.rs`, `src/cli/twin.rs`,
//! `src/cli/pdf.rs`, and the `tests/twin_*.rs` / `tests/pdf_*.rs` integration tests
//! (the twin + PDF port).

pub mod docx;
pub mod pptx;

use crate::ast::Warning;
use crate::error::{Error, Result};

/// What `twin` keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwinOptions {
    /// Extract embedded images (`--no-images` turns this off: prose and tables only).
    pub images: bool,
    /// The folder name media files are referenced under, relative to the Markdown
    /// (`media` → `![alt](media/image1.png)`).
    pub media_dir: String,
}

impl Default for TwinOptions {
    fn default() -> Self {
        TwinOptions {
            images: true,
            media_dir: "media".to_string(),
        }
    }
}

/// A Markdown twin and the files it references.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Twin {
    /// The Markdown text.
    pub markdown: String,
    /// The extracted media: path relative to the Markdown, and bytes.
    pub media: Vec<(String, Vec<u8>)>,
    /// What could not be carried over faithfully.
    pub warnings: Vec<Warning>,
}

/// The kind of document a byte buffer holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A Word document.
    Docx,
    /// A PowerPoint deck.
    Pptx,
    /// A PDF.
    Pdf,
}

/// Read a document back into Markdown, whatever its kind.
///
/// # Errors
///
/// [`Error::Unsupported`] for a PDF (out of scope without OCR) and, today, for every
/// format — the readers are not ported yet.
pub fn to_markdown(bytes: &[u8], kind: SourceKind, options: &TwinOptions) -> Result<Twin> {
    match kind {
        SourceKind::Docx => docx::to_markdown(bytes, options),
        SourceKind::Pptx => pptx::to_markdown(bytes, options),
        SourceKind::Pdf => Err(Error::unsupported(
            "PDF → Markdown: reading a PDF back needs text extraction with layout analysis \
             (and OCR for scans), which md2star-rs does not do; convert the source \
             .docx/.pptx instead",
        )),
    }
}
