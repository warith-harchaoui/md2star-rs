//! The BibTeX parser (see [`crate::bib`] for what it must accept).

use super::Bibliography;
use crate::error::{Error, Result};

/// Parse BibTeX source into a [`Bibliography`].
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — the parser is not ported yet. Once ported:
/// [`Error::Bib`] on input that cannot be read as BibTeX, naming the line.
pub fn parse(source: &str) -> Result<Bibliography> {
    let _ = source;
    Err(Error::unsupported("BibTeX parsing"))
}
