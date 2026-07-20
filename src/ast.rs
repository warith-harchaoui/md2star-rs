//! The intermediate representation (IR) that sits between the reader and the writer.
//!
//! This is the whole point of the architecture: `pulldown-cmark` emits a *flat* stream of
//! start/end events, and `docx-rs` wants a *tree* of paragraphs and runs. Rather than wire
//! one directly to the other, we fold events into these owned [`Block`]/[`Inline`] trees
//! ([`crate::reader`]) and then walk the tree to emit OOXML ([`crate::writer`]). Keeping a
//! named IR in the middle is exactly Pandoc's reader→AST→writer seam — it is what lets a
//! second backend (a Typst or HTML writer) be added later without touching the reader.

/// A run of inline (character-level) content inside a block.
///
/// Formatting nests: `**bold _and italic_**` parses to `Strong([Text, Emph([Text])])`, and
/// the writer flattens that nesting into `docx-rs` runs carrying the accumulated flags.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Literal text.
    Text(String),
    /// Emphasis (`*x*` / `_x_`) → italic.
    Emph(Vec<Inline>),
    /// Strong emphasis (`**x**`) → bold.
    Strong(Vec<Inline>),
    /// Inline code (`` `x` ``) → a monospace run.
    Code(String),
    /// A hyperlink. v0.1 renders only the visible `text`; the `url` is retained in the IR
    /// so a real OOXML hyperlink relation can be emitted later without a reader change.
    Link {
        /// The visible, possibly-formatted link text.
        text: Vec<Inline>,
        /// The destination URL.
        url: String,
    },
    /// A soft line break (a newline in the source) — rendered as a space.
    SoftBreak,
    /// A hard line break (two trailing spaces / `\`) — rendered as a line break run.
    HardBreak,
    /// A footnote reference (`[^label]`). v0.1 renders the marker inline; see the module
    /// docs in [`crate::writer`] for the "real Word footnotes" follow-up.
    FootnoteRef(String),
    /// An image. v0.1 keeps only the alt text as a placeholder; embedding the media part
    /// is a documented follow-up.
    Image(String),
}

/// A block-level element. A document is just a `Vec<Block>`.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A heading, `level` in `1..=6`.
    Heading {
        /// Heading depth, clamped to `1..=6`.
        level: u8,
        /// The heading's inline content.
        content: Vec<Inline>,
    },
    /// A paragraph of inline content.
    Paragraph(Vec<Inline>),
    /// A fenced or indented code block. `lang` is the info string if any.
    CodeBlock {
        /// The fence info string (e.g. `rust`), if present.
        lang: Option<String>,
        /// The verbatim code, newlines preserved.
        code: String,
    },
    /// A block quote — its inner blocks, rendered recursively.
    BlockQuote(Vec<Block>),
    /// A bullet or ordered list. Each item is itself a `Vec<Block>` (an item can hold a
    /// paragraph plus a nested list, etc.).
    List {
        /// `true` for ordered (`1.`), `false` for bullet (`-`).
        ordered: bool,
        /// The first number of an ordered list (usually 1); ignored for bullets.
        start: u64,
        /// One entry per list item.
        items: Vec<Vec<Block>>,
    },
    /// A GFM pipe table: a header row plus zero or more body rows, each a vector of cells.
    Table {
        /// The header cells.
        headers: Vec<Vec<Inline>>,
        /// The body rows, each a vector of cells.
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A thematic break (`---`).
    ThematicBreak,
    /// A footnote definition, split out of the body and rendered in a trailing "Notes"
    /// section by the writer.
    FootnoteDef {
        /// The footnote label that references point at.
        label: String,
        /// The note's block content.
        content: Vec<Block>,
    },
}
