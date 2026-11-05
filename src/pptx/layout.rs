//! Slide segmentation and deterministic layout selection.
//!
//! # What this module must do
//!
//! Port `md2star/pptx_layout.py`'s deterministic half:
//!
//! - **Segmentation** ([`segment`]): every thematic break ends a slide, every `#`/`##`
//!   heading starts one (both, always); speaker notes ([`Block::Notes`]) go to the slide
//!   they follow; a front-matter `title` that no slide carries becomes the cover; the
//!   cover's subtitle falls back to the front matter's `subtitle`, `author`, `date`.
//! - **Role** ([`choose_role`]): an explicit `{layout="Name"}` wins; then a class
//!   (`.statement`/`.big`, `.bignumber`, `.quote`, `.recap`, `.agenda`, `.section`,
//!   `.closing`, `.cover`, `.photo`); then the slide's form — the first title-only slide
//!   is the cover, the last the closing, a title-only `#` in between a section; a short
//!   bold figure alone is a big number; a quote with no list is a quote; one short
//!   sentence is a statement; a lone figure is a figure slide; `::: columns` is two
//!   columns; anything else is content. A recap title ("What we learned", "Ce que nous
//!   avons appris") is a recap.
//! - **Fit** (`--min-pt`): no text under N points — move to a roomier layout or split
//!   the slide into "(cont.)" / "(suite)" slides instead of shrinking.
//!
//! Model-driven selection (`--smart-layout`) calls `src/llm.rs` behind the `llm`
//! feature and only ever overrides this module's choice; it never replaces it.
//!
//! Owned by the PPTX port (see [`crate::pptx`]).

use crate::ast::{Attr, Block, Document, Inline};
use crate::error::{Error, Result};

/// One slide, cut out of the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// 1-based position in the deck.
    pub index: usize,
    /// The slide title (the heading's inlines; empty for an untitled slide).
    pub title: Vec<Inline>,
    /// The heading level that opened the slide (0 when none did).
    pub level: u8,
    /// The heading's attributes (`{.statement layout="Big Number"}`).
    pub attr: Attr,
    /// What is drawn on the slide, notes removed.
    pub body: Vec<Block>,
    /// The speaker notes' blocks.
    pub notes: Vec<Block>,
}

/// The role a slide plays, which picks its layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// An explicit `{layout="Name"}`: that template layout, by name.
    Named(String),
    /// The title slide.
    Cover,
    /// A section divider.
    Section,
    /// One short sentence, set large.
    Statement,
    /// A key figure, set very large.
    BigNumber,
    /// A quotation.
    Quote,
    /// A recap of what came before.
    Recap,
    /// An agenda.
    Agenda,
    /// The closing slide.
    Closing,
    /// A lone figure.
    Figure,
    /// Two columns (`::: columns`).
    TwoColumn,
    /// Ordinary content: title and body.
    Content,
}

/// Cut a document into slides (see the module docs).
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — segmentation is not ported yet.
pub fn segment(doc: &Document) -> Result<Vec<Slide>> {
    let _ = doc;
    Err(Error::unsupported("PPTX slide segmentation"))
}

/// The role of `slide` in a deck of `slide_count` slides (see the module docs).
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — layout selection is not ported yet.
pub fn choose_role(slide: &Slide, slide_count: usize) -> Result<Role> {
    let _ = (slide, slide_count);
    Err(Error::unsupported("PPTX layout selection"))
}
