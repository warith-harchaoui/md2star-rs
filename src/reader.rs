//! Markdown reader: `pulldown-cmark` events → our [`Block`]/[`Inline`] AST.
//!
//! `pulldown-cmark` hands us a *flat* stream — `Start(tag)`, text, `End(tag)` — where the
//! nesting is implicit in the order. We rebuild the tree with a cursor and mutual
//! recursion: [`inlines`] consumes one inline container up to its closing `End`, and
//! [`blocks`] consumes block containers, recursing into [`inlines`] for their content. The
//! invariant that keeps this simple: every `Start` has exactly one matching `End`, so a
//! function that "consumes until its End" always leaves the cursor on the next sibling.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ast::{Block, Inline};

/// A rewindable-by-one cursor over the fully-collected event stream.
///
/// We materialise the events into a `Vec` so we can `peek` the next one without fighting
/// the borrow checker over a lazy iterator; the streams are small (one document) so the
/// allocation is a non-issue.
struct Cursor<'a> {
    events: Vec<Event<'a>>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Return the next event and advance, or `None` at end of stream.
    fn next(&mut self) -> Option<Event<'a>> {
        let ev = self.events.get(self.pos).cloned();
        // Only advance when there was something to take, so repeated calls at EOF are safe.
        if ev.is_some() {
            self.pos += 1;
        }
        ev
    }

    /// Look at the next event without consuming it.
    fn peek(&self) -> Option<&Event<'a>> {
        self.events.get(self.pos)
    }
}

/// Parse a Markdown string into a document (a list of blocks).
///
/// GFM tables, footnotes, strikethrough and task lists are enabled so the reader covers
/// the same surface most md2star users write today.
///
/// # Examples
///
/// ```
/// let doc = md2star_rs::reader::parse("# Title\n\nHello **world**.");
/// assert_eq!(doc.len(), 2); // the heading and the paragraph
/// ```
pub fn parse(markdown: &str) -> Vec<Block> {
    // Match Pandoc-ish expectations: tables + footnotes + strikethrough + task lists.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Collect once, then walk with the cursor.
    let events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let mut cursor = Cursor { events, pos: 0 };
    blocks(&mut cursor)
}

/// Consume block-level content until the stream ends or the *enclosing* container closes.
///
/// On an `End`, we `break` *without* consuming it: the caller that opened the container is
/// responsible for eating its own `End`, which keeps the nesting bookkeeping in one place.
fn blocks(cursor: &mut Cursor) -> Vec<Block> {
    let mut out = Vec::new();
    while let Some(event) = cursor.peek() {
        match event {
            // A new block container opens: take the tag, then dispatch on it.
            Event::Start(_) => {
                // The peek proved this is a Start, so the `next`/`match` cannot fail.
                if let Some(Event::Start(tag)) = cursor.next() {
                    if let Some(block) = container(cursor, tag) {
                        out.push(block);
                    }
                }
            }
            // Our enclosing container is closing — hand control back to the opener.
            Event::End(_) => break,
            // A horizontal rule is a standalone block with no children.
            Event::Rule => {
                cursor.next();
                out.push(Block::ThematicBreak);
            }
            // Stray inline/HTML at the top level (rare): skip it rather than mis-nest.
            _ => {
                cursor.next();
            }
        }
    }
    out
}

/// Build one block from its just-consumed opening `tag`, eating the matching `End`.
///
/// Returns `None` for containers we intentionally drop at block level (there are none
/// today, but the shape keeps future "ignore this" cases honest).
fn container(cursor: &mut Cursor, tag: Tag) -> Option<Block> {
    match tag {
        // Headings and paragraphs are pure inline content; `inlines` eats their End.
        Tag::Heading { level, .. } => Some(Block::Heading {
            level: heading_level(level),
            content: inlines(cursor),
        }),
        Tag::Paragraph => Some(Block::Paragraph(inlines(cursor))),

        // A code block is a run of Text events terminated by its End.
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(info) if !info.is_empty() => Some(info.to_string()),
                _ => None,
            };
            let mut code = String::new();
            loop {
                match cursor.next() {
                    Some(Event::Text(t)) => code.push_str(&t),
                    Some(Event::End(_)) | None => break,
                    // Ignore anything else that can appear inside a fence.
                    _ => {}
                }
            }
            Some(Block::CodeBlock { lang, code })
        }

        // Block quote: recurse for the inner blocks, then swallow the End(BlockQuote).
        Tag::BlockQuote(_) => {
            let inner = blocks(cursor);
            cursor.next(); // consume End(BlockQuote)
            Some(Block::BlockQuote(inner))
        }

        // A list owns its items; each Item's inner blocks come from `blocks`.
        Tag::List(first) => {
            let ordered = first.is_some();
            let start = first.unwrap_or(1);
            let mut items = Vec::new();
            loop {
                match cursor.next() {
                    // `blocks` stops on End(Item); the loop's next iteration eats that End.
                    Some(Event::Start(Tag::Item)) => items.push(blocks(cursor)),
                    Some(Event::End(TagEnd::List(_))) | None => break,
                    // End(Item) and task-list markers fall through here and are consumed.
                    _ => {}
                }
            }
            Some(Block::List {
                ordered,
                start,
                items,
            })
        }

        // A GFM table: a head row then body rows, each parsed cell-by-cell.
        Tag::Table(_) => {
            let mut headers = Vec::new();
            let mut rows = Vec::new();
            loop {
                match cursor.next() {
                    Some(Event::Start(Tag::TableHead)) => headers = table_row(cursor),
                    Some(Event::Start(Tag::TableRow)) => rows.push(table_row(cursor)),
                    Some(Event::End(TagEnd::Table)) | None => break,
                    _ => {}
                }
            }
            Some(Block::Table { headers, rows })
        }

        // A footnote definition: its label plus recursively-parsed block content.
        Tag::FootnoteDefinition(label) => {
            let content = blocks(cursor);
            cursor.next(); // consume End(FootnoteDefinition)
            Some(Block::FootnoteDef {
                label: label.to_string(),
                content,
            })
        }

        // Any other block-ish start we don't model yet: parse its children so the cursor
        // stays balanced, and surface them as a plain paragraph rather than losing them.
        _ => {
            let content = inlines(cursor);
            if content.is_empty() {
                None
            } else {
                Some(Block::Paragraph(content))
            }
        }
    }
}

/// Consume one row's cells until the row's `End` (End(TableHead)/End(TableRow)).
fn table_row(cursor: &mut Cursor) -> Vec<Vec<Inline>> {
    let mut cells = Vec::new();
    loop {
        match cursor.next() {
            // `inlines` consumes the End(TableCell) itself.
            Some(Event::Start(Tag::TableCell)) => cells.push(inlines(cursor)),
            Some(Event::End(_)) | None => break,
            _ => {}
        }
    }
    cells
}

/// Consume inline content up to and *including* the closing `End` of the current container.
///
/// Nested inline containers (emphasis inside strong, etc.) recurse, each eating their own
/// `End`, so a single `End` here always means "my container is done".
fn inlines(cursor: &mut Cursor) -> Vec<Inline> {
    let mut out = Vec::new();
    loop {
        match cursor.next() {
            None => break,
            // The one End that belongs to this container — stop, already consumed.
            Some(Event::End(_)) => break,
            Some(Event::Text(t)) => out.push(Inline::Text(t.to_string())),
            Some(Event::Code(t)) => out.push(Inline::Code(t.to_string())),
            Some(Event::SoftBreak) => out.push(Inline::SoftBreak),
            Some(Event::HardBreak) => out.push(Inline::HardBreak),
            Some(Event::FootnoteReference(label)) => {
                out.push(Inline::FootnoteRef(label.to_string()))
            }
            Some(Event::Start(Tag::Emphasis)) => out.push(Inline::Emph(inlines(cursor))),
            Some(Event::Start(Tag::Strong)) => out.push(Inline::Strong(inlines(cursor))),
            // Strikethrough isn't modelled as its own run yet; keep the words, drop the
            // styling by splicing its inner inlines transparently.
            Some(Event::Start(Tag::Strikethrough)) => out.extend(inlines(cursor)),
            Some(Event::Start(Tag::Link { dest_url, .. })) => out.push(Inline::Link {
                text: inlines(cursor),
                url: dest_url.to_string(),
            }),
            // Image alt text lives in the inner inlines; flatten it to a placeholder.
            Some(Event::Start(Tag::Image { .. })) => {
                let alt = plain_text(&inlines(cursor));
                out.push(Inline::Image(alt));
            }
            // Any other inline container: recurse to stay balanced, ignore the wrapper.
            Some(Event::Start(_)) => {
                let _ = inlines(cursor);
            }
            // Task-list markers, raw HTML, math, etc.: skipped in v0.1.
            Some(_) => {}
        }
    }
    out
}

/// Flatten an inline slice to its bare text — used for image alt placeholders.
fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => s.push_str(t),
            Inline::Emph(inner) | Inline::Strong(inner) => s.push_str(&plain_text(inner)),
            Inline::Link { text, .. } => s.push_str(&plain_text(text)),
            Inline::SoftBreak => s.push(' '),
            _ => {}
        }
    }
    s
}

/// Map `pulldown-cmark`'s `HeadingLevel` enum to a clamped `1..=6` depth.
fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
