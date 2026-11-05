//! Templates compiled into the binary, for `md2star-rs templates`.
//!
//! # What this module must do
//!
//! List the reference templates md2star-rs carries (a `.docx` and a `.pptx`, from the
//! Python md2star's `data/`) so a conversion has a good default without any file or
//! network access, and print or extract one on demand. The Python version also resolves
//! per-project and cached templates; that lookup is a CLI concern and may be ported, the
//! remote default template (`deraison.ai/template.*`) is not — no network by default.
//!
//! Owned by the PPTX port (see [`crate::pptx`]); the DOCX port registers its own
//! template here too.

use crate::error::{Error, Result};
use crate::options::Format;

/// A template compiled into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledTemplate {
    /// A short name (`default`).
    pub name: &'static str,
    /// The format it styles.
    pub format: Format,
    /// The file's bytes.
    pub bytes: &'static [u8],
}

/// Every bundled template.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — no template is bundled yet.
pub fn bundled() -> Result<Vec<BundledTemplate>> {
    Err(Error::unsupported("bundled templates"))
}
