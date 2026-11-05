//! DOCX → Markdown (see [`crate::twin`] for what it must carry over).

use super::{Twin, TwinOptions};
use crate::error::{Error, Result};

/// Read a `.docx` back into Markdown.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — not ported yet. Once ported: [`Error::Read`]
/// when the bytes are not a readable `.docx`.
pub fn to_markdown(docx: &[u8], options: &TwinOptions) -> Result<Twin> {
    let _ = (docx, options);
    Err(Error::unsupported("twin for DOCX"))
}
