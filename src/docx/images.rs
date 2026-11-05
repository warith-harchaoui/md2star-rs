//! Images embedded in a Word document.
//!
//! # What this module must do
//!
//! Turn an [`crate::ast::Image`] into an inline `<w:drawing>` run: load the bytes with
//! [`crate::resources::Resolver`], size the picture with
//! [`crate::resources::fit_emu`] (the image's own pixel size at 96 dpi, the author's
//! `width=`/`height=` attributes, then scaled down to the text column), add the media
//! part and its relationship, and set the alt text (`descr`) from the image's alt.
//! An image that cannot be loaded becomes a warning and a visible placeholder, never a
//! failed conversion. Figures ([`crate::ast::Block::Figure`]) get a caption paragraph
//! in the template's `Caption` style.
//!
//! Owned by the DOCX port (see [`crate::docx`]).

use crate::ast::Attr;
use crate::error::{Error, Result};
use crate::resources::LoadedImage;

/// Build the run that shows `image` at the size `attr` asks for, inside a text column of
/// `max_width_emu` × `max_height_emu`.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — image embedding is not ported yet.
pub fn image_run(
    image: &LoadedImage,
    attr: &Attr,
    max_width_emu: u64,
    max_height_emu: u64,
) -> Result<docx_rs::Run> {
    let _ = (image, attr, max_width_emu, max_height_emu);
    Err(Error::unsupported("images in DOCX"))
}
