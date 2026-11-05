//! Citation and reference-list rendering (see [`crate::bib`] for the styles).

use super::{Bibliography, Style};
use crate::ast::{Document, Warning};
use crate::error::{Error, Result};

/// See [`crate::bib::apply`].
pub(super) fn apply(
    doc: &Document,
    bib: &Bibliography,
    style: Style,
    title: &str,
) -> Result<(Document, Vec<Warning>)> {
    let _ = (doc, bib, style, title);
    Err(Error::unsupported("citation rendering"))
}
