//! DOCX writer: our [`Block`]/[`Inline`] AST → a `docx-rs` [`Docx`].
//!
//! This is the half that replaces Pandoc's OOXML writer. Because we build every run and
//! paragraph ourselves, we *own* the output — there is no post-hoc `styles.xml` surgery
//! (the reason the Python md2star ships `postprocess.py`).
//!
//! Since v0.2 the writer is stateful (a [`Writer`]) so it can emit two features that need
//! document-level bookkeeping:
//! - **Native Word list numbering** — real `numbering.xml` levels + `numPr` on paragraphs,
//!   not a glyph typed into the text. Each ordered list gets its own numbering instance so
//!   it restarts at 1; nesting maps to indent levels.
//! - **Real Word footnotes** — `Run::add_footnote_reference`, which `docx-rs` collects into
//!   `word/footnotes.xml`, instead of an inline `[n]` marker plus a trailing section.
//!
//! Determinism / idempotence: footnote ids come from our own per-document counter (not
//! `Footnote::new`'s process-global one) and no timestamps are written, so the same
//! Markdown yields byte-identical `.docx` output on every run — see `tests/convert.rs`.
//!
//! Remaining v0.1 limitations still open (tracked follow-ups): links/images render
//! text/alt only; block quotes recurse without an indent/border; footnote references
//! inside table cells fall back to a text marker (`docx-rs` only collects footnotes from
//! top-level document paragraphs); custom ordered-list start numbers render from 1.

use std::collections::HashMap;

use docx_rs::{
    AbstractNumbering, Docx, Footnote, IndentLevel, Level, LevelJc, LevelText, NumberFormat,
    Numbering, NumberingId, Paragraph, Run, RunFonts, SpecialIndentType, Start, Table, TableCell,
    TableRow,
};

use crate::ast::{Block, Inline};

/// Font used for inline code and code blocks.
const MONO_FONT: &str = "Consolas";
/// Abstract numbering id for bullet lists (defined once in [`Writer::new`]).
const BULLET_ABSTRACT: usize = 0;
/// Abstract numbering id for ordered/decimal lists.
const DECIMAL_ABSTRACT: usize = 1;
/// The single numbering *instance* shared by every bullet list (bullets don't count, so
/// sharing one instance is harmless and keeps the numbering part small).
const BULLET_NUM_ID: usize = 1;
/// Word supports nine list levels (0..=8); deeper nesting is clamped to the last.
const MAX_LEVEL: usize = 8;

/// Build a complete [`Docx`] from a parsed document.
///
/// Footnote *definitions* are indexed up front and pulled in at each reference site, so a
/// definition may sit anywhere in the source relative to its reference(s).
pub fn build(blocks: &[Block]) -> Docx {
    let mut writer = Writer::new();
    // Index footnote definitions up front so a reference can precede its definition.
    writer.index_footnotes(blocks);
    for block in blocks {
        // Definitions are not rendered inline; they are inlined at their reference.
        if !matches!(block, Block::FootnoteDef { .. }) {
            writer.write_block(block, 0);
        }
    }
    writer.finish()
}

/// Holds the document under construction plus the counters that make list numbering and
/// footnote ids deterministic.
struct Writer {
    /// `Option` so builder methods (which consume `self`) can be threaded through `take`.
    docx: Option<Docx>,
    /// Next ordered-list numbering instance id to hand out (bullets reuse [`BULLET_NUM_ID`]).
    next_num_id: usize,
    /// Next footnote id — our own counter, so output is reproducible run to run.
    next_footnote_id: usize,
    /// Footnote label → its definition's block content.
    footnote_defs: HashMap<String, Vec<Block>>,
}

impl Writer {
    /// Create the document and register the two abstract numberings (bullet + decimal) plus
    /// the shared bullet instance.
    fn new() -> Self {
        let docx = Docx::new()
            .add_abstract_numbering(bullet_abstract())
            .add_abstract_numbering(decimal_abstract())
            .add_numbering(Numbering::new(BULLET_NUM_ID, BULLET_ABSTRACT));
        Writer {
            docx: Some(docx),
            // Ordered-list instances start after the reserved bullet instance.
            next_num_id: BULLET_NUM_ID + 1,
            next_footnote_id: 1,
            footnote_defs: HashMap::new(),
        }
    }

    /// Index a document's footnote definitions before rendering references.
    fn index_footnotes(&mut self, blocks: &[Block]) {
        for block in blocks {
            if let Block::FootnoteDef { label, content } = block {
                self.footnote_defs.insert(label.clone(), content.clone());
            }
        }
    }

    /// Consume the writer and return the finished document.
    fn finish(mut self) -> Docx {
        self.docx.take().expect("docx is present until finish")
    }

    /// Append a paragraph to the document body.
    fn push_paragraph(&mut self, paragraph: Paragraph) {
        let docx = self.docx.take().expect("docx present");
        self.docx = Some(docx.add_paragraph(paragraph));
    }

    /// Append a table to the document body.
    fn push_table(&mut self, table: Table) {
        let docx = self.docx.take().expect("docx present");
        self.docx = Some(docx.add_table(table));
    }

    /// Register a numbering instance (used per ordered list so each restarts at 1).
    fn register_numbering(&mut self, numbering: Numbering) {
        let docx = self.docx.take().expect("docx present");
        self.docx = Some(docx.add_numbering(numbering));
    }

    /// Allocate the next ordered-list numbering instance id.
    fn alloc_num_id(&mut self) -> usize {
        let id = self.next_num_id;
        self.next_num_id += 1;
        id
    }

    /// Allocate the next (deterministic) footnote id.
    fn alloc_footnote_id(&mut self) -> usize {
        let id = self.next_footnote_id;
        self.next_footnote_id += 1;
        id
    }

    /// Emit one block. `depth` is the current list-nesting level (0 at the top).
    fn write_block(&mut self, block: &Block, depth: usize) {
        match block {
            Block::Heading { level, content } => {
                let size = heading_half_points(*level);
                let mut paragraph = Paragraph::new();
                // Headings are bold; carry the level's size onto each run.
                for run in self.inline_runs(content, true, false, true) {
                    paragraph = paragraph.add_run(run.size(size));
                }
                self.push_paragraph(paragraph);
            }
            Block::Paragraph(inlines) => {
                let paragraph = self.paragraph(inlines, false, false, true);
                self.push_paragraph(paragraph);
            }
            Block::CodeBlock { code, .. } => self.write_code_block(code),
            // Block quotes recurse (no indent/border yet — a tracked follow-up).
            Block::BlockQuote(inner) => {
                for child in inner {
                    self.write_block(child, depth);
                }
            }
            Block::List {
                ordered,
                start,
                items,
            } => self.write_list(*ordered, *start, items, depth),
            Block::Table { headers, rows } => {
                let table = self.table(headers, rows);
                self.push_table(table);
            }
            Block::ThematicBreak => {
                self.push_paragraph(
                    Paragraph::new().add_run(Run::new().add_text("————————————————")),
                );
            }
            // Definitions are inlined at their reference; a stray one renders as text.
            Block::FootnoteDef { label, .. } => {
                self.push_paragraph(
                    Paragraph::new().add_run(Run::new().add_text(format!("[{label}]"))),
                );
            }
        }
    }

    /// Emit a list with **native Word numbering**.
    ///
    /// Ordered lists each get a fresh numbering instance (so they restart at 1); bullet
    /// lists share one. Nesting maps to the paragraph's indent level. Each item's first
    /// paragraph carries the number/bullet; a nested list inside an item recurses one level
    /// deeper.
    fn write_list(&mut self, ordered: bool, _start: u64, items: &[Vec<Block>], depth: usize) {
        // Pick (or mint) the numbering instance for this list.
        let num_id = if ordered {
            let id = self.alloc_num_id();
            self.register_numbering(Numbering::new(id, DECIMAL_ABSTRACT));
            id
        } else {
            BULLET_NUM_ID
        };
        let level = depth.min(MAX_LEVEL);

        for item in items {
            // The item's first paragraph becomes the numbered line.
            if let Some(Block::Paragraph(inlines)) =
                item.iter().find(|b| matches!(b, Block::Paragraph(_)))
            {
                let mut paragraph =
                    Paragraph::new().numbering(NumberingId::new(num_id), IndentLevel::new(level));
                for run in self.inline_runs(inlines, false, false, true) {
                    paragraph = paragraph.add_run(run);
                }
                self.push_paragraph(paragraph);
            }
            // Nested lists inside the item recurse one indent level deeper.
            for child in item {
                if let Block::List {
                    ordered,
                    start,
                    items,
                } = child
                {
                    self.write_list(*ordered, *start, items, depth + 1);
                }
            }
        }
    }

    /// Emit a code block as one monospace paragraph per line so breaks survive.
    fn write_code_block(&mut self, code: &str) {
        // `lines()` drops a trailing newline; an empty block yields no paragraphs.
        for line in code.lines() {
            self.push_paragraph(Paragraph::new().add_run(mono(line)));
        }
    }

    /// Build a plain paragraph from inline content.
    fn paragraph(
        &mut self,
        inlines: &[Inline],
        bold: bool,
        italic: bool,
        footnotes: bool,
    ) -> Paragraph {
        let mut paragraph = Paragraph::new();
        for run in self.inline_runs(inlines, bold, italic, footnotes) {
            paragraph = paragraph.add_run(run);
        }
        paragraph
    }

    /// Flatten an inline tree into `docx-rs` runs, threading emphasis flags through nesting.
    ///
    /// When `footnotes` is true, a [`Inline::FootnoteRef`] with a known definition becomes a
    /// real Word footnote reference; otherwise (table cells, footnote bodies) it renders as
    /// a `[label]` text marker, because `docx-rs` only collects footnotes from top-level
    /// paragraphs and a marker there would otherwise orphan.
    fn inline_runs(
        &mut self,
        inlines: &[Inline],
        bold: bool,
        italic: bool,
        footnotes: bool,
    ) -> Vec<Run> {
        let mut out = Vec::new();
        for inline in inlines {
            match inline {
                Inline::Text(t) => out.push(styled(t, bold, italic)),
                // Nesting flips one flag and recurses; leaves become runs.
                Inline::Emph(inner) => out.extend(self.inline_runs(inner, bold, true, footnotes)),
                Inline::Strong(inner) => {
                    out.extend(self.inline_runs(inner, true, italic, footnotes))
                }
                Inline::Code(t) => out.push(mono(t)),
                // v0.1: a link contributes only its (formatted) visible text.
                Inline::Link { text, .. } => {
                    out.extend(self.inline_runs(text, bold, italic, footnotes))
                }
                Inline::SoftBreak => out.push(styled(" ", bold, italic)),
                Inline::HardBreak => {
                    out.push(Run::new().add_break(docx_rs::BreakType::TextWrapping))
                }
                Inline::FootnoteRef(label) => {
                    out.push(self.footnote_run(label, bold, italic, footnotes))
                }
                Inline::Image(alt) => out.push(styled(&format!("[image: {alt}]"), bold, italic)),
            }
        }
        out
    }

    /// Build the run for a footnote reference — a real Word footnote when possible.
    fn footnote_run(&mut self, label: &str, bold: bool, italic: bool, footnotes: bool) -> Run {
        // Clone the definition out first so the following `&mut self` calls don't overlap.
        if footnotes {
            if let Some(content) = self.footnote_defs.get(label).cloned() {
                let id = self.alloc_footnote_id();
                // Build the footnote with our own deterministic id via a struct literal —
                // `Footnote::new()` would pull a non-reproducible process-global id instead.
                let mut note = Footnote {
                    id,
                    content: Vec::new(),
                };
                for paragraph in self.footnote_content(&content) {
                    note.add_content(paragraph);
                }
                return Run::new().add_footnote_reference(note);
            }
        }
        // No live footnote (undefined label, or a cell/body context): fall back to a marker.
        styled(&format!("[{label}]"), bold, italic)
    }

    /// Render a footnote definition's blocks into paragraphs for `footnotes.xml`.
    ///
    /// Footnotes here are rendered without *nested* footnotes (`footnotes = false`), which
    /// keeps ids linear and avoids a footnote-inside-a-footnote edge case.
    fn footnote_content(&mut self, blocks: &[Block]) -> Vec<Paragraph> {
        let mut paragraphs = Vec::new();
        for block in blocks {
            if let Block::Paragraph(inlines) = block {
                paragraphs.push(self.paragraph(inlines, false, false, false));
            }
        }
        // A footnote must have at least one paragraph or Word complains; guarantee one.
        if paragraphs.is_empty() {
            paragraphs.push(Paragraph::new());
        }
        paragraphs
    }

    /// Build a `docx-rs` [`Table`] from header + body cells.
    fn table(&mut self, headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> Table {
        let mut table_rows = Vec::new();

        // Header row is bold to read like a GFM header. Cells use plain (marker) footnotes.
        let header_cells: Vec<TableCell> = headers
            .iter()
            .map(|cell| TableCell::new().add_paragraph(self.paragraph(cell, true, false, false)))
            .collect();
        table_rows.push(TableRow::new(header_cells));

        for row in rows {
            let cells: Vec<TableCell> = row
                .iter()
                .map(|cell| {
                    TableCell::new().add_paragraph(self.paragraph(cell, false, false, false))
                })
                .collect();
            table_rows.push(TableRow::new(cells));
        }

        Table::new(table_rows)
    }
}

// Free helpers below don't need writer state.

/// Build the abstract numbering for bullet lists: nine levels of alternating glyphs.
fn bullet_abstract() -> AbstractNumbering {
    let mut abstract_num = AbstractNumbering::new(BULLET_ABSTRACT);
    for level in 0..=MAX_LEVEL {
        // Cycle filled/hollow/square bullets so nested levels read differently.
        let glyph = match level % 3 {
            0 => "●",
            1 => "○",
            _ => "▪",
        };
        abstract_num = abstract_num.add_level(indented_level(
            level,
            NumberFormat::new("bullet"),
            LevelText::new(glyph),
        ));
    }
    abstract_num
}

/// Build the abstract numbering for ordered lists: nine decimal levels (`%1.`, `%2.`, …).
fn decimal_abstract() -> AbstractNumbering {
    let mut abstract_num = AbstractNumbering::new(DECIMAL_ABSTRACT);
    for level in 0..=MAX_LEVEL {
        // `%N` references the counter of the Nth level, so each depth prints its own number.
        let text = format!("%{}.", level + 1);
        abstract_num = abstract_num.add_level(indented_level(
            level,
            NumberFormat::new("decimal"),
            LevelText::new(text),
        ));
    }
    abstract_num
}

/// A single numbering level with a hanging indent that grows with depth.
fn indented_level(level: usize, format: NumberFormat, text: LevelText) -> Level {
    // 720 twips (~0.5in) of indent per level, with a 360-twip hanging indent for the marker.
    let left = 720 * (level as i32 + 1);
    Level::new(level, Start::new(1), format, text, LevelJc::new("left")).indent(
        Some(left),
        Some(SpecialIndentType::Hanging(360)),
        None,
        None,
    )
}

/// Half-point font size for a heading level (`size()` is in half-points, so 40 → 20pt).
fn heading_half_points(level: u8) -> usize {
    match level {
        1 => 40,
        2 => 32,
        3 => 28,
        4 => 26,
        5 => 24,
        _ => 22,
    }
}

/// A text run with the requested emphasis applied.
fn styled(text: &str, bold: bool, italic: bool) -> Run {
    let mut run = Run::new().add_text(text);
    if bold {
        run = run.bold();
    }
    if italic {
        run = run.italic();
    }
    run
}

/// A monospace run for inline code / code-block lines.
fn mono(text: &str) -> Run {
    Run::new()
        .add_text(text)
        .fonts(RunFonts::new().ascii(MONO_FONT).hi_ansi(MONO_FONT))
}
