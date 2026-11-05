//! The conversion contract shared by every backend: what a caller can ask for
//! ([`Options`]) and what it gets back ([`Rendered`]).
//!
//! Ownership: shared foundation. Every field the Python `md2star` CLI exposes with a
//! meaning outside Pandoc is laid out here up front, so the DOCX, PPTX and PDF backends
//! can each honour their part without editing this file. A backend that does not honour
//! a field yet says so with a `not-yet-ported` [`Warning`] — it never ignores it silently.
//!
//! Everything is in memory (bytes in, bytes out): an iOS or Android host hands over
//! buffers, and only [`Options::base_dir`] / [`Options::bibliography`] touch the
//! filesystem, through [`crate::resources`].

use std::path::PathBuf;

use crate::ast::Warning;

/// The output format of a conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Microsoft Word.
    Docx,
    /// Microsoft PowerPoint.
    Pptx,
    /// Portable Document Format (feature `pdf`).
    Pdf,
}

impl Format {
    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Pptx => "pptx",
            Format::Pdf => "pdf",
        }
    }
}

/// Conversion options, aligned on `md2star docx|pptx|pdf --help`.
///
/// `Options::default()` is a plain conversion: no template, no bibliography, local
/// images only, no network.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Options {
    /// The directory relative image and bibliography paths resolve against (the input
    /// document's directory). `None` means the current directory.
    pub base_dir: Option<PathBuf>,
    /// `--author`: overrides the front matter's `author`.
    pub author: Option<String>,
    /// `--date`: overrides the front matter's `date`, used verbatim.
    pub date: Option<String>,
    /// `--lang` (BCP 47): overrides the front matter's `lang`.
    pub lang: Option<String>,
    /// `--bib`: BibTeX files, added to the front matter's `bibliography`.
    pub bibliography: Vec<PathBuf>,
    /// `--bibliography-name`: the heading before the bibliography (default: localised
    /// "References" / "Références").
    pub bibliography_name: Option<String>,
    /// The citation style: `author-date` (default) or `numeric`.
    pub citation_style: Option<String>,
    /// `--reference-doc`: the raw bytes of a `.docx` (for DOCX/PDF) or `.pptx` (for PPTX)
    /// whose styles, masters and layouts the output inherits.
    pub reference_doc: Option<Vec<u8>>,
    /// `--toc`: insert a table of contents.
    pub toc: bool,
    /// `--allow-remote-images`: fetch `http(s)://` images (needs the `net` feature).
    pub allow_remote_images: bool,
    /// `--offline`: forbid every network access; wins over `allow_remote_images`.
    pub offline: bool,
    /// `--min-pt` (PPTX): never set slide text under this many points — change layout or
    /// split the slide instead of shrinking.
    pub min_pt: Option<f32>,
    /// `--smart-layout` (PPTX): pick layouts with a language model (feature `llm`).
    /// Without it, layout selection is deterministic.
    pub smart_layout: bool,
    /// The OpenAI-compatible endpoint for `smart_layout` (best-engine-ai-helper serves
    /// `/v1`). `None` falls back to the `MD2STAR_LLM_ENDPOINT` environment variable.
    pub llm_endpoint: Option<String>,
    /// The model name for `smart_layout`; `None` lets the endpoint choose (`best`).
    pub llm_model: Option<String>,
}

impl Options {
    /// Whether a remote image may be fetched: asked for, and not overridden by
    /// `offline`.
    pub fn remote_images_allowed(&self) -> bool {
        self.allow_remote_images && !self.offline
    }
}

/// A finished conversion: the file's bytes and what the converter had to say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rendered {
    /// The complete output file.
    pub bytes: Vec<u8>,
    /// Non-fatal observations (an image that could not be loaded, a Mermaid diagram left
    /// as code, an option this backend does not honour yet), in the order they arose.
    pub warnings: Vec<Warning>,
}
