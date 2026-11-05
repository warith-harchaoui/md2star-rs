//! PowerPoint templates: reading a `.pptx`'s masters and layouts, and the templates
//! bundled in the binary.
//!
//! # What this module must do
//!
//! Read a reference/designer `.pptx` (pure XML over the zip, no Office, no Python): its
//! slide size, its layouts by name with their placeholders (type, index, box in EMU) and
//! the declared text sizes, so [`crate::pptx::layout`] can map roles onto real layouts and
//! the writer can build slides *on* those layouts (reusing the template's masters, theme
//! and layouts rather than a generic scaffold). Also lists the templates compiled into
//! the binary for `md2star-rs templates`.
//!
//! Owned by the PPTX port (see [`crate::pptx`]).

use crate::error::{Error, Result};

/// One placeholder of a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    /// The placeholder type (`title`, `ctrTitle`, `body`, `pic`, `subTitle`, …).
    pub kind: String,
    /// Its `idx`.
    pub idx: u32,
    /// Its box, `(x, y, cx, cy)` in EMU, when the layout (or its master) declares one.
    pub frame: Option<(i64, i64, i64, i64)>,
}

/// One named layout of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    /// The layout's name as PowerPoint shows it (`Title and Content`, `Big Number`…).
    pub name: String,
    /// The part path inside the zip (`ppt/slideLayouts/slideLayout2.xml`).
    pub part: String,
    /// Its placeholders.
    pub placeholders: Vec<Placeholder>,
}

/// What a template offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInfo {
    /// Slide width and height in EMU.
    pub slide_size: (i64, i64),
    /// The layouts, in master order.
    pub layouts: Vec<LayoutInfo>,
}

/// Read a template's slide size and layouts.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — template reading is not ported yet.
pub fn inspect(pptx: &[u8]) -> Result<TemplateInfo> {
    let _ = pptx;
    Err(Error::unsupported("reading PPTX templates"))
}
