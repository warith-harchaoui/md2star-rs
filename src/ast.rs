//! The intermediate representation (IR) that sits between the reader and every writer.
//!
//! `pulldown-cmark` emits a *flat* stream of start/end events and each backend wants a
//! *tree*. Rather than wire one directly to the other, [`crate::reader`] folds the events
//! into the owned [`Document`] / [`Block`] / [`Inline`] trees below, and each writer walks
//! that tree. This is Pandoc's reader → AST → writer seam: the DOCX, PPTX, PDF and twin
//! code all read this one vocabulary, so a construction the reader learns is available to
//! every backend at once.
//!
//! Ownership: this file and [`crate::reader`] form the shared foundation. Writers consume
//! the AST; they do not change it. A construction a writer cannot render yet is skipped
//! or rendered as text by that writer — it is never dropped here.
//!
//! What the tree keeps that a renderer might not: the raw LaTeX of every formula, the
//! source of every image, Pandoc attributes on headings, code blocks, divs, images, links
//! and tables, citation keys with their locators, and speaker notes kept apart from the
//! slide body. Losing any of these at read time would make the downstream feature
//! impossible without re-reading the Markdown.

use std::collections::BTreeMap;

/// A parsed Markdown document: its front-matter metadata, its body, and what the reader
/// had to say about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// The YAML front matter, typed where md2star gives a key a meaning, free otherwise.
    pub meta: Metadata,
    /// The body, in source order.
    pub blocks: Vec<Block>,
    /// Non-fatal observations made while reading (an unclosed `:::` div, an unparsable
    /// front matter). The CLI prints them; a library caller may surface or ignore them.
    pub warnings: Vec<Warning>,
}

/// A non-fatal diagnostic. Shared by the reader and every writer, so a caller gets one
/// kind of message whatever produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// A short stable identifier (`"unclosed-div"`, `"mermaid-not-rendered"`, …) a caller
    /// can match on without parsing the message.
    pub code: &'static str,
    /// The human-readable explanation, in English.
    pub message: String,
}

impl Warning {
    /// Build a warning from its code and message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Warning {
            code,
            message: message.into(),
        }
    }
}

/// Front-matter metadata.
///
/// The keys md2star gives a meaning to are lifted into typed fields; **every** key,
/// typed or not, also stays in [`Metadata::raw`] so nothing a template or a later
/// feature might want is lost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    /// `title`.
    pub title: Option<String>,
    /// `subtitle`.
    pub subtitle: Option<String>,
    /// `author` — a string, a list of strings, or a list of `{name: …}` maps, flattened
    /// to the names in order.
    pub authors: Vec<String>,
    /// `date`, verbatim.
    pub date: Option<String>,
    /// `date_format` (md2star's strftime pattern for an auto-generated date).
    pub date_format: Option<String>,
    /// `lang` (BCP 47, e.g. `fr-FR`).
    pub lang: Option<String>,
    /// `abstract`.
    pub abstract_text: Option<String>,
    /// `keywords` — a string or a list.
    pub keywords: Vec<String>,
    /// `bibliography` — one path or a list, as written (relative to the document).
    pub bibliography: Vec<String>,
    /// `csl`, as written. Only its *name* can steer the built-in styles; full CSL
    /// processing is out of scope (see [`crate::bib`]).
    pub csl: Option<String>,
    /// `citation-style` or `style` (md2star's short name: `author-date`, `numeric`, …).
    pub citation_style: Option<String>,
    /// `reference-section-title` (Pandoc) or `bibliography-name` (md2star): the heading
    /// placed before the bibliography.
    pub reference_section_title: Option<String>,
    /// `toc` / `table-of-contents`.
    pub toc: Option<bool>,
    /// `md2star_skip` — preprocessing phases the author asked to skip.
    pub skip: Vec<String>,
    /// Every key of the front matter, typed ones included, in key order.
    pub raw: BTreeMap<String, MetaValue>,
}

/// A front-matter value, kept structurally (the "free map" half of [`Metadata`]).
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    /// A string scalar (also what YAML numbers and dates are kept as, verbatim).
    String(String),
    /// A boolean scalar.
    Bool(bool),
    /// A sequence.
    List(Vec<MetaValue>),
    /// A mapping, in key order.
    Map(BTreeMap<String, MetaValue>),
    /// `null` / `~` / an empty value.
    Null,
}

impl MetaValue {
    /// The value as a string when it is a scalar (string or bool), `None` otherwise.
    pub fn as_str(&self) -> Option<String> {
        match self {
            MetaValue::String(s) => Some(s.clone()),
            MetaValue::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

/// Pandoc attributes: `{#id .class key="value"}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attr {
    /// `#id`, if any. Headings get an automatic one (Pandoc's `auto_identifiers`).
    pub id: Option<String>,
    /// `.class` entries, in order, without the dot.
    pub classes: Vec<String>,
    /// `key=value` / `key="value with spaces"` pairs, in order.
    pub kv: Vec<(String, String)>,
}

impl Attr {
    /// Is `class` among this element's classes?
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// The value of the first `key=…` attribute, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.kv
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// True when nothing is set.
    pub fn is_empty(&self) -> bool {
        self.id.is_none() && self.classes.is_empty() && self.kv.is_empty()
    }
}

/// An image reference, inline or as a figure.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// The source exactly as written: a relative or absolute path, a `data:` URI or a
    /// remote URL. Resolution is [`crate::resources`]' job, not the reader's.
    pub src: String,
    /// The alt text as inlines (it may carry emphasis, math or a citation); see
    /// [`Image::alt_text`] for the flattened form.
    pub alt: Vec<Inline>,
    /// The optional title (`![a](x.png "title")`).
    pub title: Option<String>,
    /// Trailing attributes (`{width=50% #fig:x}`).
    pub attr: Attr,
}

impl Image {
    /// The alt text flattened to plain text (what a screen reader would announce).
    pub fn alt_text(&self) -> String {
        plain_text(&self.alt)
    }
}

/// How a citation is rendered relative to the surrounding prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationMode {
    /// `[@key]` — parenthetical: "(Pearl, 2009)".
    Normal,
    /// `@key` in running text — "Pearl (2009)".
    AuthorInText,
    /// `[-@key]` — the year only: "(2009)".
    SuppressAuthor,
}

/// One cited work inside a citation group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// The BibTeX key, without the `@`.
    pub key: String,
    /// Text before the key inside the brackets (`see` in `[see @key]`).
    pub prefix: String,
    /// Text after the key (`p. 3` in `[@key, p. 3]`), the leading comma removed.
    pub suffix: String,
    /// Rendering mode.
    pub mode: CitationMode,
}

/// A run of inline (character-level) content inside a block.
///
/// Formatting nests: `**bold _and italic_**` parses to `Strong([Text, Emph([Text])])`.
/// Adjacent text is merged by the reader, so a writer never sees two `Text` in a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Literal text.
    Text(String),
    /// Emphasis (`*x*` / `_x_`).
    Emph(Vec<Inline>),
    /// Strong emphasis (`**x**`).
    Strong(Vec<Inline>),
    /// Strikeout (`~~x~~`).
    Strikeout(Vec<Inline>),
    /// Superscript (`^x^`).
    Superscript(Vec<Inline>),
    /// Subscript (`~x~`).
    Subscript(Vec<Inline>),
    /// Inline code (`` `x` ``).
    Code(String),
    /// A formula. `tex` is the raw LaTeX between the delimiters, untouched.
    Math {
        /// `true` for `$$…$$` (display), `false` for `$…$` (inline).
        display: bool,
        /// The LaTeX source without its delimiters.
        tex: String,
    },
    /// A hyperlink.
    Link {
        /// The visible, possibly-formatted link text.
        text: Vec<Inline>,
        /// The destination URL (or `#anchor`).
        url: String,
        /// The optional title.
        title: Option<String>,
        /// Trailing attributes (`[x](y){.class}`).
        attr: Attr,
    },
    /// An inline image.
    Image(Image),
    /// A citation group: `[@a, p. 3; @b]` or an in-text `@a`.
    Cite(Vec<Citation>),
    /// A footnote reference (`[^label]`); the definition is a [`Block::FootnoteDef`].
    FootnoteRef(String),
    /// A Pandoc inline note (`^[text]`): the note's content, in place.
    InlineNote(Vec<Inline>),
    /// Raw inline HTML (`<br>`, `<!-- … -->`), kept verbatim; writers may ignore it.
    RawHtml(String),
    /// A soft line break (a newline in the source) — usually rendered as a space.
    SoftBreak,
    /// A hard line break (two trailing spaces / `\`).
    HardBreak,
}

/// Horizontal alignment of a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// No alignment marker (`---`).
    #[default]
    Default,
    /// `:---`.
    Left,
    /// `:---:`.
    Center,
    /// `---:`.
    Right,
}

/// One item of a list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `Some(checked)` for a task-list item (`- [x]` / `- [ ]`), `None` otherwise.
    pub task: Option<bool>,
    /// The item's blocks (its text, then any nested list or paragraph).
    pub content: Vec<Block>,
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A heading.
    Heading {
        /// Depth, `1..=6`.
        level: u8,
        /// The heading's inline content, attribute block removed.
        content: Vec<Inline>,
        /// `{#id .class key="v"}`; `id` is filled automatically when absent.
        attr: Attr,
    },
    /// A paragraph of inline content.
    Paragraph(Vec<Inline>),
    /// A fenced or indented code block (Mermaid blocks are [`Block::Mermaid`]).
    CodeBlock {
        /// The language: the info string's first word, or the first class of a
        /// `{.python}` attribute block.
        lang: Option<String>,
        /// The verbatim code, newlines preserved.
        code: String,
        /// Attributes from the info string (`{.numberLines #c1}`).
        attr: Attr,
    },
    /// A Mermaid diagram (```` ```mermaid ```` or a code block with class `mermaid`).
    Mermaid {
        /// The diagram source.
        code: String,
        /// Attributes from the info string.
        attr: Attr,
    },
    /// An image alone in its paragraph: Pandoc's implicit figure. The caption is the
    /// image's alt text ([`Image::alt`]), as in Pandoc.
    Figure(Image),
    /// A block quote.
    BlockQuote(Vec<Block>),
    /// A bullet or ordered list.
    List {
        /// `true` for ordered (`1.`), `false` for bullets (`-`).
        ordered: bool,
        /// The first number of an ordered list (usually 1); ignored for bullets.
        start: u64,
        /// The items.
        items: Vec<ListItem>,
    },
    /// A GFM pipe table.
    Table {
        /// The header cells.
        headers: Vec<Vec<Inline>>,
        /// The body rows.
        rows: Vec<Vec<Vec<Inline>>>,
        /// One alignment per column.
        aligns: Vec<Align>,
        /// The caption from a `Table: …` / `: …` paragraph next to the table.
        caption: Vec<Inline>,
        /// Attributes trailing the caption (`Table: x {#tbl:y}`).
        attr: Attr,
    },
    /// A fenced div (`::: {.columns}` … `:::`), other than speaker notes.
    Div {
        /// The div's attributes (`::: warning` gives the class `warning`).
        attr: Attr,
        /// Its blocks.
        content: Vec<Block>,
    },
    /// Speaker notes (`::: notes` … `:::`, as in Pandoc). Never drawn on a slide.
    Notes(Vec<Block>),
    /// A thematic break (`---`, `***`, `___`). In a deck it is a slide separator.
    ThematicBreak,
    /// Raw HTML at block level, kept verbatim (comments included).
    RawHtml(String),
    /// A footnote definition (`[^label]: …`).
    FootnoteDef {
        /// The label references point at.
        label: String,
        /// The note's blocks.
        content: Vec<Block>,
    },
}

/// A citation group spelled back as Markdown source (`[see @a, p. 3; -@b]`, `@a [p. 3]`).
///
/// What a writer shows for a citation it cannot resolve: the reader of the output sees
/// exactly what the author wrote rather than nothing.
pub fn cite_text(cites: &[Citation]) -> String {
    if let [only] = cites {
        if only.mode == CitationMode::AuthorInText {
            return if only.suffix.is_empty() {
                format!("@{}", only.key)
            } else {
                format!("@{} [{}]", only.key, only.suffix)
            };
        }
    }
    let items: Vec<String> = cites
        .iter()
        .map(|c| {
            let mut item = String::new();
            if !c.prefix.is_empty() {
                item.push_str(&c.prefix);
                item.push(' ');
            }
            if c.mode == CitationMode::SuppressAuthor {
                item.push('-');
            }
            item.push('@');
            item.push_str(&c.key);
            if !c.suffix.is_empty() {
                item.push_str(", ");
                item.push_str(&c.suffix);
            }
            item
        })
        .collect();
    format!("[{}]", items.join("; "))
}

/// Flatten inlines to the text a reader would see (formatting removed, notes dropped,
/// math kept as its LaTeX source, images as their alt text, citations as `@key`).
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    push_plain(inlines, &mut s);
    s
}

fn push_plain(inlines: &[Inline], s: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => s.push_str(t),
            Inline::Math { tex, .. } => s.push_str(tex),
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner) => push_plain(inner, s),
            Inline::Link { text, .. } => push_plain(text, s),
            Inline::Image(image) => push_plain(&image.alt, s),
            Inline::Cite(cites) => {
                let keys: Vec<String> = cites.iter().map(|c| format!("@{}", c.key)).collect();
                s.push_str(&keys.join("; "));
            }
            Inline::SoftBreak | Inline::HardBreak => s.push(' '),
            Inline::FootnoteRef(_) | Inline::InlineNote(_) | Inline::RawHtml(_) => {}
        }
    }
}
