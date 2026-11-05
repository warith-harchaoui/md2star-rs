//! What a slide carries, in the units the fit model measures and the writer places.
//!
//! The AST is a document's tree; a slide wants a flat sequence of things to set:
//! paragraphs at a list level with or without a bullet or a number, tables, code
//! listings and pictures. [`units`] flattens a slide's blocks into that sequence, and
//! [`columns`] recognises Pandoc's `:::: columns` / `::: column`. Every unit can be
//! measured ([`super::fit`]) and split between slides at its own boundary (a list
//! between items, a table between rows, a listing between lines), which is what lets
//! `--min-pt` split a slide instead of shrinking it.

use std::collections::BTreeMap;

use crate::ast::{cite_text, Align, Block, Image, Inline};
use crate::resources::LoadedImage;

/// How a paragraph is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bullet {
    /// Prose: no glyph, no indent.
    None,
    /// A bullet-list item.
    Char,
    /// An ordered-list item, with its number.
    Number(u64),
    /// A task-list item, checked or not.
    Task(bool),
}

/// One paragraph to set.
#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    /// The inline content.
    pub inlines: Vec<Inline>,
    /// The list depth (0 = top level).
    pub level: usize,
    /// The list marker.
    pub bullet: Bullet,
    /// Set in italics (a quotation).
    pub italic: bool,
    /// Set in bold (a run-in heading inside a slide).
    pub bold: bool,
}

impl Para {
    /// Plain prose at level 0.
    pub fn plain(inlines: Vec<Inline>) -> Self {
        Para {
            inlines,
            level: 0,
            bullet: Bullet::None,
            italic: false,
            bold: false,
        }
    }

    /// The text this paragraph prints (what is measured).
    pub fn text(&self) -> String {
        let mut s = match self.bullet {
            Bullet::Task(true) => "☑ ".to_string(),
            Bullet::Task(false) => "☐ ".to_string(),
            _ => String::new(),
        };
        s.push_str(&printed(&self.inlines));
        s
    }
}

/// A table to set.
#[derive(Debug, Clone, PartialEq)]
pub struct TableUnit {
    /// The header cells.
    pub headers: Vec<Vec<Inline>>,
    /// The body rows.
    pub rows: Vec<Vec<Vec<Inline>>>,
    /// One alignment per column.
    pub aligns: Vec<Align>,
}

impl TableUnit {
    /// The number of columns.
    pub fn cols(&self) -> usize {
        self.rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(self.headers.len()))
            .max()
            .unwrap_or(0)
    }
}

/// A picture to place.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// The image as written.
    pub image: Image,
    /// The key of its bytes in the deck's image store (its `src`).
    pub key: String,
    /// Width over height.
    pub aspect: f64,
}

/// One unit of slide content.
#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    /// A paragraph.
    Para(Para),
    /// A table.
    Table(TableUnit),
    /// A code listing: its lines, tabs expanded.
    Code(Vec<String>),
    /// A picture.
    Picture(Picture),
}

impl Unit {
    /// Whether this unit is text set in a text box (a paragraph).
    pub fn is_para(&self) -> bool {
        matches!(self, Unit::Para(_))
    }
}

/// The images of a deck, loaded once, by `src`.
pub type ImageStore = BTreeMap<String, LoadedImage>;

/// Footnote definitions, by label, in the order they are numbered.
#[derive(Debug, Clone, Default)]
pub struct Footnotes {
    /// Label → (number, content).
    pub defs: BTreeMap<String, (usize, Vec<Block>)>,
}

/// The text a run of inlines prints: formatting dropped, citations as written, a note
/// reference as its number, math as its LaTeX, an image as its alt text.
pub fn printed(inlines: &[Inline]) -> String {
    let mut s = String::new();
    push_printed(inlines, &mut s);
    s
}

fn push_printed(inlines: &[Inline], s: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => s.push_str(t),
            Inline::Math { tex, .. } => s.push_str(tex),
            Inline::Emph(i)
            | Inline::Strong(i)
            | Inline::Strikeout(i)
            | Inline::Superscript(i)
            | Inline::Subscript(i) => push_printed(i, s),
            Inline::Link { text, .. } => push_printed(text, s),
            Inline::Image(img) => push_printed(&img.alt, s),
            Inline::Cite(c) => s.push_str(&cite_text(c)),
            Inline::FootnoteRef(label) => {
                s.push('[');
                s.push_str(label);
                s.push(']');
            }
            Inline::InlineNote(_) => s.push_str("[*]"),
            Inline::SoftBreak => s.push(' '),
            Inline::HardBreak => s.push('\n'),
            Inline::RawHtml(_) => {}
        }
    }
}

/// Is this paragraph only pictures (and spaces between them)?
fn only_images(inlines: &[Inline]) -> Option<Vec<Image>> {
    let mut images = Vec::new();
    for i in inlines {
        match i {
            Inline::Image(img) => images.push(img.clone()),
            Inline::SoftBreak | Inline::HardBreak => {}
            Inline::Text(t) if t.trim().is_empty() => {}
            _ => return None,
        }
    }
    (!images.is_empty()).then_some(images)
}

/// The picture unit for `image`, or the alt text in italics when it did not load.
fn picture(image: &Image, store: &ImageStore) -> Unit {
    match store.get(&image.src) {
        Some(loaded) => Unit::Picture(Picture {
            image: image.clone(),
            key: image.src.clone(),
            aspect: loaded.info.aspect_ratio().max(0.01),
        }),
        None => {
            let mut alt = image.alt.clone();
            if alt.is_empty() {
                alt.push(Inline::Text(image.src.clone()));
            }
            Unit::Para(Para {
                italic: true,
                ..Para::plain(alt)
            })
        }
    }
}

/// Every image a list of blocks references (figures and inline images), in order.
pub fn images_in(blocks: &[Block], out: &mut Vec<Image>) {
    fn inl(inlines: &[Inline], out: &mut Vec<Image>) {
        for i in inlines {
            match i {
                Inline::Image(img) => out.push(img.clone()),
                Inline::Emph(x)
                | Inline::Strong(x)
                | Inline::Strikeout(x)
                | Inline::Superscript(x)
                | Inline::Subscript(x)
                | Inline::InlineNote(x) => inl(x, out),
                Inline::Link { text, .. } => inl(text, out),
                _ => {}
            }
        }
    }
    for b in blocks {
        match b {
            Block::Figure(img) => out.push(img.clone()),
            Block::Paragraph(i) => inl(i, out),
            Block::Heading { content, .. } => inl(content, out),
            Block::BlockQuote(x) | Block::Div { content: x, .. } | Block::Notes(x) => {
                images_in(x, out)
            }
            Block::List { items, .. } => {
                for it in items {
                    images_in(&it.content, out);
                }
            }
            Block::FootnoteDef { content, .. } => images_in(content, out),
            _ => {}
        }
    }
}

/// Flatten blocks into units.
pub fn units(blocks: &[Block], store: &ImageStore) -> Vec<Unit> {
    let mut out = Vec::new();
    for b in blocks {
        push_block(b, 0, false, store, &mut out);
    }
    out
}

fn push_block(b: &Block, level: usize, italic: bool, store: &ImageStore, out: &mut Vec<Unit>) {
    match b {
        Block::Paragraph(inlines) => {
            if let Some(images) = only_images(inlines) {
                for img in &images {
                    out.push(picture(img, store));
                }
            } else {
                out.push(Unit::Para(Para {
                    italic,
                    level,
                    ..Para::plain(inlines.clone())
                }));
            }
        }
        Block::Heading { content, .. } => out.push(Unit::Para(Para {
            bold: true,
            level,
            ..Para::plain(content.clone())
        })),
        Block::Figure(img) => {
            let unit = picture(img, store);
            let loaded = matches!(unit, Unit::Picture(_));
            out.push(unit);
            // Pandoc's implicit figure: the alt text is the caption, set under it.
            if loaded && !img.alt.is_empty() {
                out.push(Unit::Para(Para {
                    italic: true,
                    level,
                    ..Para::plain(img.alt.clone())
                }));
            }
        }
        Block::CodeBlock { code, .. } | Block::Mermaid { code, .. } => {
            out.push(Unit::Code(code_lines(code)));
        }
        Block::BlockQuote(inner) => {
            for x in inner {
                push_block(x, level, true, store, out);
            }
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            for (k, item) in items.iter().enumerate() {
                let bullet = match (item.task, ordered) {
                    (Some(done), _) => Bullet::Task(done),
                    (None, true) => Bullet::Number(start + k as u64),
                    (None, false) => Bullet::Char,
                };
                let mut first = true;
                for inner in &item.content {
                    match inner {
                        Block::Paragraph(inlines) if first => {
                            out.push(Unit::Para(Para {
                                inlines: inlines.clone(),
                                level,
                                bullet,
                                italic,
                                bold: false,
                            }));
                            first = false;
                        }
                        Block::List { .. } => push_block(inner, level + 1, italic, store, out),
                        other => {
                            if first {
                                // An item that opens with something other than text still
                                // gets its marker, on an empty line of its own.
                                out.push(Unit::Para(Para {
                                    inlines: Vec::new(),
                                    level,
                                    bullet,
                                    italic,
                                    bold: false,
                                }));
                                first = false;
                            }
                            push_block(other, level + 1, italic, store, out);
                        }
                    }
                }
                if first {
                    out.push(Unit::Para(Para {
                        inlines: Vec::new(),
                        level,
                        bullet,
                        italic,
                        bold: false,
                    }));
                }
            }
        }
        Block::Table {
            headers,
            rows,
            aligns,
            caption,
            ..
        } => {
            out.push(Unit::Table(TableUnit {
                headers: headers.clone(),
                rows: rows.clone(),
                aligns: aligns.clone(),
            }));
            if !caption.is_empty() {
                out.push(Unit::Para(Para {
                    italic: true,
                    ..Para::plain(caption.clone())
                }));
            }
        }
        Block::Div { content, .. } => {
            for x in content {
                push_block(x, level, italic, store, out);
            }
        }
        Block::Notes(_) | Block::ThematicBreak | Block::RawHtml(_) | Block::FootnoteDef { .. } => {}
    }
}

/// A listing's lines, tabs expanded to four spaces, trailing newline dropped.
pub fn code_lines(code: &str) -> Vec<String> {
    let code = code.strip_suffix('\n').unwrap_or(code);
    if code.is_empty() {
        return vec![String::new()];
    }
    code.lines().map(|l| l.replace('\t', "    ")).collect()
}

/// `:::: columns` with `::: column` children: each column's blocks. `None` when the
/// slide is not laid out in columns.
pub fn columns(blocks: &[Block]) -> Option<Vec<Vec<Block>>> {
    let mut found = None;
    for b in blocks {
        if let Block::Div { attr, content } = b {
            if attr.has_class("columns") {
                let cols: Vec<Vec<Block>> = content
                    .iter()
                    .filter_map(|c| match c {
                        Block::Div { attr, content } if attr.has_class("column") => {
                            Some(content.clone())
                        }
                        _ => None,
                    })
                    .collect();
                if cols.len() >= 2 {
                    found = Some(cols);
                }
            }
        }
    }
    found
}

/// Split the units of one flow at `n` points of content: the boundaries the fit model
/// may split a slide at. Lists split between items (a nested item stays with its
/// parent), tables between rows, listings between lines.
pub fn split_points(units: &[Unit]) -> Vec<Vec<Unit>> {
    let mut chunks: Vec<Vec<Unit>> = Vec::new();
    for u in units {
        match u {
            Unit::Para(p) if p.level > 0 && !chunks.is_empty() => {
                // A sub-item never leaves its parent.
                if let Some(last) = chunks.last_mut() {
                    last.push(u.clone());
                }
            }
            Unit::Table(t) if t.rows.len() > 1 => {
                for row in &t.rows {
                    chunks.push(vec![Unit::Table(TableUnit {
                        headers: t.headers.clone(),
                        rows: vec![row.clone()],
                        aligns: t.aligns.clone(),
                    })]);
                }
            }
            Unit::Code(lines) if lines.len() > 1 => {
                for l in lines {
                    chunks.push(vec![Unit::Code(vec![l.clone()])]);
                }
            }
            _ => chunks.push(vec![u.clone()]),
        }
    }
    chunks
}

/// Join adjacent chunks back into units: consecutive table rows of one table become one
/// table again, consecutive code lines one listing.
pub fn rejoin(chunks: &[Vec<Unit>]) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for u in chunks.iter().flatten() {
        match (out.last_mut(), u) {
            (Some(Unit::Table(a)), Unit::Table(b))
                if a.headers == b.headers && b.rows.len() == 1 =>
            {
                a.rows.extend(b.rows.iter().cloned());
            }
            (Some(Unit::Code(a)), Unit::Code(b)) if b.len() == 1 => a.extend(b.iter().cloned()),
            _ => out.push(u.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;

    #[test]
    fn lists_flatten_to_levels_and_numbers() {
        let blocks = parse("- a\n  - b\n- c\n\n3. x\n4. y\n\n- [x] done\n");
        let u = units(&blocks, &ImageStore::new());
        let paras: Vec<(usize, Bullet, String)> = u
            .iter()
            .filter_map(|u| match u {
                Unit::Para(p) => Some((p.level, p.bullet, p.text())),
                _ => None,
            })
            .collect();
        assert_eq!(paras[0], (0, Bullet::Char, "a".into()));
        assert_eq!(paras[1], (1, Bullet::Char, "b".into()));
        assert_eq!(paras[3], (0, Bullet::Number(3), "x".into()));
        assert_eq!(paras[4], (0, Bullet::Number(4), "y".into()));
        assert_eq!(paras[5], (0, Bullet::Task(true), "☑ done".into()));
    }

    #[test]
    fn an_unloadable_image_becomes_its_alt_text() {
        let blocks = parse("![A chart](missing.png)\n");
        let u = units(&blocks, &ImageStore::new());
        assert!(matches!(&u[0], Unit::Para(p) if p.italic && p.text() == "A chart"));
    }

    #[test]
    fn columns_are_recognised() {
        let blocks = parse(":::: columns\n::: column\nLeft\n:::\n::: column\nRight\n:::\n::::\n");
        let cols = columns(&blocks).unwrap();
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn splitting_keeps_sub_items_with_their_parent_and_rejoins_tables() {
        let blocks = parse("- a\n  - a1\n- b\n\n| h |\n|---|\n| 1 |\n| 2 |\n");
        let u = units(&blocks, &ImageStore::new());
        let chunks = split_points(&u);
        assert_eq!(chunks[0].len(), 2, "a and its sub-item a1 stay together");
        let back = rejoin(&chunks);
        assert_eq!(back, u);
    }

    #[test]
    fn code_lines_expand_tabs() {
        assert_eq!(code_lines("a\n\tb\n"), vec!["a", "    b"]);
    }
}
