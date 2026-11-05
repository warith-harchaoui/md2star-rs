//! Citations and the bibliography, as they appear in a Word document.
//!
//! The format-independent work — reading the `.bib`, formatting a citation in the
//! author-date or numeric style, ordering the reference list — is [`crate::bib`]'s. This
//! is the DOCX side: [`apply`] runs [`crate::bib::resolve`] with the conversion's
//! options; the reference list it returns is a `Div {#refs}` of
//! `Div {#ref-KEY custom-style=Bibliography}` entries, which the writer renders in the
//! reference document's `Bibliography` paragraph style (hanging indent in ours), each
//! entry carrying a bookmark `ref-KEY` that a `[x](#ref-KEY)` link can jump to.
//!
//! Owned by the DOCX port (see [`crate::docx`]).

use crate::ast::{Document, Warning};
use crate::error::Result;
use crate::options::Options;

/// Resolve every [`crate::ast::Inline::Cite`] of `doc` against the bibliographies named
/// by `options.bibliography` and the front matter's `bibliography`, and append the
/// reference list. Returns the rewritten document and the warnings (unknown keys, an
/// unknown citation style, an unreadable `.bib`…). Without a bibliography the document
/// is returned as is.
///
/// # Errors
///
/// None today (see [`crate::bib::resolve`]).
pub fn apply(doc: &Document, options: &Options) -> Result<(Document, Vec<Warning>)> {
    crate::bib::resolve(doc, options)
}
