//! The v0.4 PPTX writer, on `ppt-rs` (feature `legacy-pptx`, on by default).
//!
//! Each `#` heading starts a slide titled after it; everything until the next `#` is
//! flattened to bullets (paragraphs and list items, nested lists and deeper headings as
//! sub-bullets, code lines, one bullet per table row). Inline formatting is flattened to
//! text: the `ppt-rs` quick-slide API is text-only.
//!
//! Why it is quarantined behind a feature: `ppt-rs` depends on `zip` 0.6 with its
//! default features, which compile bzip2 and zstd **from C**. That C is what kept the
//! core from building for iOS and Android without a cross C toolchain. The PPTX port
//! replaces this file with a pure-Rust PresentationML writer built on the template's own
//! masters and layouts — which `ppt-rs` cannot do anyway — and then the feature and the
//! dependency go.

use std::io::{Cursor, Read, Write};

use ppt_rs::pptx;
use ppt_rs::prelude::SlideContent;

use crate::ast::{cite_text, Block, Inline, ListItem};
use crate::error::{Error, Result};

/// Deck title used when the document has no leading level-1 heading to name it after.
const DEFAULT_DECK_TITLE: &str = "Presentation";

/// The OPC part whose `ppt-rs`-generated wall-clock timestamps we normalise for idempotence.
const CORE_PROPS_PART: &str = "docProps/core.xml";

/// Fixed, content-independent timestamp substituted for `ppt-rs`'s `SystemTime::now()` in
/// `docProps/core.xml`. A neutral sentinel (not "now") so the same deck always serialises the
/// same bytes; the exact instant is meaningless for a generated document.
const FIXED_TIMESTAMP: &str = "2001-01-01T00:00:00Z";

/// Build a `.pptx` byte buffer from a document's blocks.
///
/// # Errors
///
/// Returns [`Error::Pptx`] if `ppt-rs` fails to assemble or pack the presentation.
pub fn build(blocks: &[Block]) -> Result<Vec<u8>> {
    let mut deck = Deck::new(deck_title(blocks));
    for block in blocks {
        match block {
            // A level-1 heading opens a fresh slide titled after it.
            Block::Heading {
                level: 1, content, ..
            } => deck.start_slide(&inline_text(content)),
            // Footnote definitions are not rendered (they have no on-slide equivalent).
            Block::FootnoteDef { .. } => {}
            // Everything else is body content on the current slide (one is opened on demand).
            other => deck.push_block(other, 0),
        }
    }
    deck.build()
}

/// The deck title: the first level-1 heading's text, or a neutral default if none leads.
///
/// Only a heading that the deck *opens with* names the whole presentation; a `#` appearing later
/// merely starts another slide, so we look at the first block only.
fn deck_title(blocks: &[Block]) -> String {
    match blocks.first() {
        Some(Block::Heading {
            level: 1, content, ..
        }) => inline_text(content),
        _ => DEFAULT_DECK_TITLE.to_string(),
    }
}

/// Accumulates slides while walking the document.
///
/// `current` is the slide being filled; it is flushed into `slides` when the next `#` arrives (or
/// at the end). Body content before any `#` opens an implicit slide titled after the deck.
struct Deck {
    /// Presentation-level title (metadata + the implicit first slide's title).
    title: String,
    /// Completed slides, in document order.
    slides: Vec<SlideContent>,
    /// The slide currently being filled, if any.
    current: Option<SlideContent>,
}

impl Deck {
    /// Start an empty deck with the given presentation title.
    fn new(title: String) -> Self {
        Deck {
            title,
            slides: Vec::new(),
            current: None,
        }
    }

    /// Flush the in-progress slide (if any) and begin a new one titled `title`.
    fn start_slide(&mut self, title: &str) {
        self.flush();
        self.current = Some(SlideContent::new(title));
    }

    /// Ensure a slide is open to receive body content, opening an implicit one titled after the
    /// deck when the document starts with body before its first `#`.
    fn ensure_slide(&mut self) {
        if self.current.is_none() {
            // Clone the title: `self` is borrowed mutably to set `current`.
            let title = self.title.clone();
            self.current = Some(SlideContent::new(&title));
        }
    }

    /// Add a top-level bullet to the current slide.
    fn bullet(&mut self, text: &str) {
        self.ensure_slide();
        // `SlideContent` builder methods consume and return self, so take/replace through Option.
        let slide = self.current.take().expect("slide is open");
        self.current = Some(slide.add_bullet(text));
    }

    /// Add a numbered bullet (for ordered-list items) to the current slide.
    fn numbered(&mut self, text: &str) {
        self.ensure_slide();
        let slide = self.current.take().expect("slide is open");
        self.current = Some(slide.add_numbered(text));
    }

    /// Add an indented sub-bullet (nested lists, deeper headings) to the current slide.
    fn sub_bullet(&mut self, text: &str) {
        self.ensure_slide();
        let slide = self.current.take().expect("slide is open");
        self.current = Some(slide.add_sub_bullet(text));
    }

    /// Render one body block onto the current slide. `depth` drives bullet vs sub-bullet.
    fn push_block(&mut self, block: &Block, depth: usize) {
        match block {
            // A paragraph is a single bullet (a sub-bullet once nested).
            Block::Paragraph(inlines) => self.bullet_at(depth, &inline_text(inlines)),
            // Deeper headings (##+) are section labels within a slide → bullets by depth.
            Block::Heading { content, .. } => self.bullet_at(depth, &inline_text(content)),
            // List items become bullets; ordered lists use numbered bullets at the top level.
            Block::List { ordered, items, .. } => self.push_list(*ordered, items, depth),
            // Block quotes contribute their inner blocks as bullets at the same depth.
            Block::BlockQuote(inner) => {
                for child in inner {
                    self.push_block(child, depth);
                }
            }
            // Each code line is its own bullet so line breaks survive the flattening.
            Block::CodeBlock { code, .. } | Block::Mermaid { code, .. } => {
                for line in code.lines() {
                    self.bullet_at(depth, line);
                }
            }
            // A table flattens to one bullet per row, cells joined by " | " (header included).
            Block::Table { headers, rows, .. } => {
                self.bullet_at(depth, &join_cells(headers));
                for row in rows {
                    self.bullet_at(depth, &join_cells(row));
                }
            }
            // A lone image is a bullet carrying its placeholder until images land.
            Block::Figure(image) => {
                self.bullet_at(depth, &inline_text(&[Inline::Image(image.clone())]))
            }
            // A div's blocks belong to the slide (columns are flattened for now).
            Block::Div { content, .. } => {
                for child in content {
                    self.push_block(child, depth);
                }
            }
            // Thematic breaks, stray footnote definitions, raw HTML and speaker notes
            // have no on-slide equivalent → skipped (notes go to the notes pane once the
            // template-based writer lands).
            Block::ThematicBreak
            | Block::FootnoteDef { .. }
            | Block::RawHtml(_)
            | Block::Notes(_) => {}
        }
    }

    /// Emit a list: each item's first paragraph is a bullet (numbered when `ordered`), and any
    /// nested list inside the item recurses one indent level deeper as sub-bullets.
    fn push_list(&mut self, ordered: bool, items: &[ListItem], depth: usize) {
        for item in items {
            // The item's lead paragraph carries the bullet text.
            if let Some(Block::Paragraph(inlines)) = item
                .content
                .iter()
                .find(|b| matches!(b, Block::Paragraph(_)))
            {
                let mut text = inline_text(inlines);
                if let Some(checked) = item.task {
                    let mark = if checked { "\u{2611} " } else { "\u{2610} " };
                    text.insert_str(0, mark);
                }
                // Ordered lists number only at the top level; nested items read as sub-bullets.
                if ordered && depth == 0 {
                    self.numbered(&text);
                } else {
                    self.bullet_at(depth, &text);
                }
            }
            // Recurse into nested lists one level deeper.
            for child in &item.content {
                if let Block::List { ordered, items, .. } = child {
                    self.push_list(*ordered, items, depth + 1);
                }
            }
        }
    }

    /// Add `text` as a top-level bullet at depth 0, or an indented sub-bullet when nested.
    fn bullet_at(&mut self, depth: usize, text: &str) {
        if depth == 0 {
            self.bullet(text);
        } else {
            self.sub_bullet(text);
        }
    }

    /// Move the in-progress slide (if any) into the completed list.
    fn flush(&mut self) {
        if let Some(slide) = self.current.take() {
            self.slides.push(slide);
        }
    }

    /// Finish the deck: flush the last slide and pack the whole presentation to `.pptx` bytes.
    fn build(mut self) -> Result<Vec<u8>> {
        self.flush();
        // Seed the quick-builder with the deck title, then hand it every accumulated slide.
        let mut builder = pptx!(&self.title);
        for slide in self.slides {
            builder = builder.content_slide(slide);
        }
        // `ppt-rs` returns its own error type; flatten it to our boundary error, then strip the
        // wall-clock timestamp so the output is byte-idempotent.
        let raw = builder.build().map_err(|e| Error::Pptx(e.to_string()))?;
        normalize(raw)
    }
}

/// Re-pack a `ppt-rs` `.pptx` so it is byte-deterministic.
///
/// `ppt-rs` writes `SystemTime::now()` into `docProps/core.xml`; two conversions a second apart
/// (or concurrent threads crossing a second boundary) would otherwise differ. We copy every part
/// through verbatim — preserving order — except `core.xml`, whose created/modified timestamps are
/// pinned to [`FIXED_TIMESTAMP`], and we stamp every entry with a fixed mtime so the zip container
/// itself carries no clock. The result is identical bytes for identical input.
fn normalize(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(&bytes)).map_err(|e| Error::Pptx(e.to_string()))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // Fixed options: a constant mtime and a stable compression method make the container itself
    // clock-free and reproducible (DEFLATE is deterministic for a given input).
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Pptx(e.to_string()))?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        // Only core.xml carries the wall-clock timestamps; everything else copies through as-is.
        if name == CORE_PROPS_PART {
            data = pin_core_timestamps(&data).into_bytes();
        }
        writer
            .start_file(name, options)
            .map_err(|e| Error::Pptx(e.to_string()))?;
        writer.write_all(&data)?;
    }
    let cursor = writer.finish().map_err(|e| Error::Pptx(e.to_string()))?;
    Ok(cursor.into_inner())
}

/// Replace the `created` and `modified` timestamps in a `core.xml` byte buffer with the fixed
/// sentinel, leaving the rest of the part untouched.
fn pin_core_timestamps(data: &[u8]) -> String {
    let xml = String::from_utf8_lossy(data);
    // Both dcterms elements hold a single timestamp text node; pin each to the sentinel.
    let pinned = replace_element_text(&xml, "dcterms:created", FIXED_TIMESTAMP);
    replace_element_text(&pinned, "dcterms:modified", FIXED_TIMESTAMP)
}

/// Return `xml` with the text content of every `<tag …>…</tag>` element replaced by `value`.
///
/// A deliberately tiny, allocation-light substitution rather than a full XML parse: `core.xml` is
/// a fixed, flat template we generated, so matching the opening tag, its `>`, and the matching
/// closing tag is sufficient and robust here.
fn replace_element_text(xml: &str, tag: &str, value: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(xml.len());
    let mut rest = xml;
    // Walk each occurrence of the element, copying everything up to its text node, then the
    // replacement, then its closing tag; anything unmatched is emitted unchanged.
    while let Some(open_at) = rest.find(&open) {
        let after_open = &rest[open_at..];
        // The element's start tag ends at the first '>' after `<tag`.
        let Some(gt) = after_open.find('>') else {
            break;
        };
        let content_start = open_at + gt + 1;
        let Some(close_rel) = rest[content_start..].find(&close) else {
            break;
        };
        let close_at = content_start + close_rel;
        result.push_str(&rest[..content_start]);
        result.push_str(value);
        result.push_str(&close);
        rest = &rest[close_at + close.len()..];
    }
    result.push_str(rest);
    result
}

/// Flatten an inline tree to plain text (bold/italic/code stripped, footnotes dropped).
///
/// Slides carry no rich inline structure through the quick API, so every inline collapses to the
/// text a reader would see: emphasis unwraps to its contents, links to their visible text, images
/// to an `[image: alt]` placeholder, and footnote references to nothing.
fn inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            // Emphasis and links contribute only their (recursively flattened) text.
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner) => out.push_str(&inline_text(inner)),
            // A formula reads as its LaTeX source until OMML lands; a citation as written.
            Inline::Math { display, tex } => {
                let fence = if *display { "$$" } else { "$" };
                out.push_str(&format!("{fence}{}{fence}", tex.trim()));
            }
            Inline::Cite(cites) => out.push_str(&cite_text(cites)),
            Inline::InlineNote(_) | Inline::RawHtml(_) => {}
            Inline::Link { text, .. } => out.push_str(&inline_text(text)),
            // Both break kinds become a space so adjacent words don't run together.
            Inline::SoftBreak | Inline::HardBreak => out.push(' '),
            // Footnotes have no place on a slide; drop the marker entirely.
            Inline::FootnoteRef(_) => {}
            Inline::Image(image) => out.push_str(&format!("[image: {}]", image.alt_text())),
        }
    }
    out
}

/// Join a row of cells into one line for a bullet, cells separated by " | ".
fn join_cells(cells: &[Vec<Inline>]) -> String {
    cells
        .iter()
        .map(|cell| inline_text(cell))
        .collect::<Vec<_>>()
        .join(" | ")
}
