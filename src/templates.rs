//! Templates compiled into the binary, for `md2star-rs templates` and as the default
//! of every conversion.
//!
//! md2star-rs carries the Python md2star's own reference templates (`data/template.pptx`
//! and `data/template.docx`, copied byte for byte into `assets/templates/`), so a
//! conversion has a good default without any file or network access. The Python version
//! also fetches a remote default (`deraison.ai/template.*`): that is not ported — no
//! network by default.
//!
//! Owned by the PPTX port (see [`crate::pptx`]); the DOCX template is registered here
//! too, so `md2star-rs templates list` shows both.

use crate::error::Result;
use crate::options::Format;

/// The default PowerPoint template: md2star's `data/template.pptx`.
pub const DEFAULT_PPTX: &[u8] = include_bytes!("../assets/templates/default.pptx");

/// The default Word template: md2star's `data/template.docx`.
pub const DEFAULT_DOCX: &[u8] = include_bytes!("../assets/templates/default.docx");

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
/// None today; the `Result` keeps room for templates read lazily.
///
/// # Examples
///
/// ```
/// use md2star_rs::{templates::bundled, Format};
/// let all = bundled().unwrap();
/// assert!(all.iter().any(|t| t.format == Format::Pptx && t.name == "default"));
/// ```
pub fn bundled() -> Result<Vec<BundledTemplate>> {
    Ok(vec![
        BundledTemplate {
            name: "default",
            format: Format::Docx,
            bytes: DEFAULT_DOCX,
        },
        BundledTemplate {
            name: "default",
            format: Format::Pptx,
            bytes: DEFAULT_PPTX,
        },
    ])
}

/// The bundled template for `format` (`None` for PDF, which has none of its own).
pub fn default_for(format: Format) -> Option<&'static [u8]> {
    match format {
        Format::Docx => Some(DEFAULT_DOCX),
        Format::Pptx => Some(DEFAULT_PPTX),
        Format::Pdf => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_templates_are_office_files() {
        for t in bundled().unwrap() {
            assert_eq!(&t.bytes[..2], b"PK", "{} {:?}", t.name, t.format);
        }
        assert_eq!(default_for(Format::Pptx), Some(DEFAULT_PPTX));
        assert_eq!(default_for(Format::Pdf), None);
    }
}
