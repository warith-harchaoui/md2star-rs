//! Citations and the bibliography, as they appear in a Word document.
//!
//! # What this module must do
//!
//! The format-independent work — parsing the `.bib`, formatting a citation in the
//! author-date or numeric style, ordering the reference list — lives in [`crate::bib`].
//! This module is the DOCX side of it: turn the resolved citations into runs (linked to
//! their bibliography entry with a bookmark when the style allows it) and emit the
//! reference list under `--bibliography-name` / `reference-section-title`, with a hanging
//! indent and the template's `Bibliography` style when it has one.
//!
//! Owned by the DOCX port (see [`crate::docx`]).

use crate::ast::{Document, Warning};
use crate::error::{Error, Result};
use crate::options::Options;

/// Resolve every [`crate::ast::Inline::Cite`] of `doc` against the bibliographies named
/// by `options.bibliography` and the front matter's `bibliography`, and append the
/// reference list. Returns the rewritten document and the warnings (unknown keys…).
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — citations are not ported yet.
pub fn apply(doc: &Document, options: &Options) -> Result<(Document, Vec<Warning>)> {
    let _ = (doc, options);
    Err(Error::unsupported("citations and bibliography in DOCX"))
}
