//! `twin`: read a finished document back into an editable Markdown twin.
//!
//! The port of `md2star twin` for Office documents, as pure XML over the zip — no
//! Office, no Python, no subprocess, so it runs wherever the library compiles:
//!
//! - **DOCX** ([`docx`]): front matter from the Title / Subtitle / Author / Date
//!   paragraphs and `docProps/core.xml`; headings (by style name, `basedOn` chain and
//!   outline level); paragraphs with bold, italic, code, strikeout, superscript and
//!   subscript runs; lists from `numbering.xml` (bullets or numbers, levels, start);
//!   pipe tables (alignment, merged cells); hyperlinks (relationships, anchors and
//!   `HYPERLINK` fields); footnotes and endnotes as `[^n]`; block quotes; code blocks;
//!   captions; OMML equations back to LaTeX for the common subset (an unknown element
//!   keeps its text and raises a warning); pictures extracted to the media folder.
//! - **PPTX** ([`pptx`]): the cover as front matter; one section per slide (`#` for a
//!   section slide, `##` otherwise), bullets with their levels, numbered lists, code,
//!   tables, pictures, two-column bodies, speaker notes as `::: notes`, `---` between
//!   slides; Pandoc's footnote slide folded back into `[^n]` definitions.
//!
//! **PDF → Markdown is out of scope**: [`to_markdown`] refuses it with an
//! [`Error::Unsupported`] that says why ([`PDF_REFUSAL`]). Re-authoring figures as
//! Mermaid/SVG (`--diagrams` in Python) needs a vision model and is not part of the
//! deterministic port. Rendered citations come back as their text: the keys are not in
//! the document.
//!
//! # Files this module owns
//!
//! `src/twin/` (this file, `docx.rs`, `pptx.rs`), `src/pdf.rs`, `src/cli/twin.rs`,
//! `src/cli/pdf.rs`, and the `tests/twin_*.rs` / `tests/pdf_*.rs` integration tests
//! (the twin + PDF port).

pub mod docx;
mod math;
mod md;
pub mod pptx;
mod xml;

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

/// OMML (an `<m:oMath>` or `<m:oMathPara>` fragment, prefixes declared or not) → LaTeX,
/// for the subset listed in the module documentation. The second value names every
/// element that had no LaTeX equivalent (its text was kept).
///
/// # Errors
///
/// [`Error::Read`] when the fragment is not well-formed XML.
///
/// # Examples
///
/// ```
/// let omml = r#"<m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#;
/// let (tex, unknown) = md2star_rs::twin::omml_to_latex(omml).unwrap();
/// assert_eq!(tex, "\\frac{a}{b}");
/// assert!(unknown.is_empty());
/// ```
pub fn omml_to_latex(omml: &str) -> Result<(String, Vec<String>)> {
    // A bare fragment uses the `m:` prefix without declaring it; quick-xml does not
    // resolve namespaces, so it parses as is.
    let root = xml::parse(omml.as_bytes())?;
    Ok(math::to_latex(&root))
}

/// Why a PDF is refused, in full: what the Python md2star does instead, why it does not
/// carry over, and what to do.
pub const PDF_REFUSAL: &str = "PDF → Markdown: the Python md2star reads a PDF back through \
     Kreuzberg (text extraction with layout analysis, plus OCR for scanned pages); \
     md2star-rs carries neither, by design (no native OCR engine on every device), so \
     run `md2star-rs twin` on the source .docx or .pptx instead";

/// Read a document back into Markdown, whatever its kind.
///
/// # Errors
///
/// [`Error::Unsupported`] for a PDF (out of scope without OCR, see [`PDF_REFUSAL`]);
/// [`Error::Read`] when a `.docx` / `.pptx` cannot be read.
pub fn to_markdown(bytes: &[u8], kind: SourceKind, options: &TwinOptions) -> Result<Twin> {
    match kind {
        SourceKind::Docx => docx::to_markdown(bytes, options),
        SourceKind::Pptx => pptx::to_markdown(bytes, options),
        SourceKind::Pdf => Err(Error::unsupported(PDF_REFUSAL)),
    }
}
