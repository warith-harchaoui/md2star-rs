//! Error type for the crate.
//!
//! The library never panics on bad input or a failed write and never prints — it returns
//! a typed [`enum@Error`] so the binary (or an iOS/Android/WASM host) decides how to
//! surface it. Non-fatal observations are not errors: they travel as
//! [`crate::ast::Warning`]s next to the output.
//!
//! Ownership: shared foundation. The variants below were laid out up front for every
//! backend (DOCX, PPTX, PDF, twin, audit, bibliography, LLM) so that the modules can be
//! completed independently without each adding its own variant here. The enum is
//! `#[non_exhaustive]`, so a variant can still be added later without breaking callers.

use thiserror::Error;

/// Everything that can go wrong in a conversion.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The input file could not be read, or the output file could not be created.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// `docx-rs` failed to serialize/zip the document.
    #[error("failed to write DOCX: {0}")]
    Docx(String),

    /// A `--reference-doc` template could not be read back as a valid document. Kept
    /// apart from [`Error::Io`] so "the template file is missing" (an `Io`) and "the file
    /// exists but is not a usable Word/PowerPoint document" (this) stay distinguishable.
    #[error("invalid reference document: {0}")]
    Template(String),

    /// The `.pptx` could not be generated or packed.
    #[error("failed to write PPTX: {0}")]
    Pptx(String),

    /// The PDF could not be generated.
    #[error("failed to write PDF: {0}")]
    Pdf(String),

    /// A feature that exists in the Python md2star but has not been ported to Rust yet,
    /// or that this build was compiled without (a disabled Cargo feature). The message
    /// says which, and what to do.
    #[error("not ported yet: {0}")]
    Unsupported(String),

    /// An image or another referenced file could not be loaded.
    #[error("cannot load resource `{src}`: {reason}")]
    Resource {
        /// The reference exactly as written in the document.
        src: String,
        /// Why it could not be loaded.
        reason: String,
    },

    /// A bibliography (`.bib`) could not be read or parsed.
    #[error("bibliography error: {0}")]
    Bib(String),

    /// A formula could not be converted (LaTeX → OMML or another math target).
    #[error("math error: {0}")]
    Math(String),

    /// An Office document read back for `twin` or `audit` is malformed (bad zip, missing
    /// part, invalid XML).
    #[error("cannot read document: {0}")]
    Read(String),

    /// The language-model endpoint (feature `llm`) failed or answered nonsense.
    #[error("language model error: {0}")]
    Llm(String),
}

impl Error {
    /// Shorthand for [`Error::Unsupported`].
    pub fn unsupported(what: impl Into<String>) -> Self {
        Error::Unsupported(what.into())
    }
}

/// Crate-wide result alias so signatures read `Result<T>` rather than the full path.
pub type Result<T> = std::result::Result<T, Error>;
