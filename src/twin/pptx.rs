//! PPTX → Markdown (see [`crate::twin`] for what it must carry over).

use super::{Twin, TwinOptions};
use crate::error::{Error, Result};

/// Read a `.pptx` back into Markdown.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — not ported yet. Once ported: [`Error::Read`]
/// when the bytes are not a readable `.pptx`.
pub fn to_markdown(pptx: &[u8], options: &TwinOptions) -> Result<Twin> {
    let _ = (pptx, options);
    Err(Error::unsupported("twin for PPTX"))
}
