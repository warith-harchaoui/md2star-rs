//! Pictures on slides.
//!
//! # What this module must do
//!
//! Place an [`crate::ast::Image`] on a slide: load it through
//! [`crate::resources::Resolver`], size it with [`crate::resources::fit_emu`] into the
//! layout's picture or body placeholder (keeping its aspect ratio, honouring
//! `width=`/`height=`), write the media part and its relationship, set the alt text, and
//! never let a picture cover text (the `audit` rule). An image that cannot be loaded is
//! a warning and a placeholder, not a failed deck.
//!
//! Owned by the PPTX port (see [`crate::pptx`]).

use crate::ast::Attr;
use crate::error::{Error, Result};
use crate::resources::LoadedImage;

/// The `<p:pic>` element for `image` inside the box `(x, y, cx, cy)` (EMU), referencing
/// the relationship `rel_id`, with shape id `shape_id`.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — slide pictures are not ported yet.
pub fn picture_xml(
    image: &LoadedImage,
    attr: &Attr,
    frame: (i64, i64, i64, i64),
    rel_id: &str,
    shape_id: u32,
) -> Result<String> {
    let _ = (image, attr, frame, rel_id, shape_id);
    Err(Error::unsupported("images in PPTX"))
}
