//! DOCX writer: our [`Block`]/[`Inline`] AST → a `docx-rs` [`Docx`].
//!
//! This is the half that replaces Pandoc's OOXML writer. Because we build every run and
//! paragraph ourselves, we *own* the output — there is no post-hoc `styles.xml` surgery
//! (the reason the Python md2star ships `postprocess.py`). The trade-off is that we only
//! emit what we explicitly handle here, so v0.1 formats headings/emphasis/code inline
//! rather than leaning on named Word styles.
//!
//! Documented v0.1 limitations (each a clean follow-up, none a redesign):
//! - **Footnotes** render as inline `[label]` markers plus a trailing "Notes" section, not
//!   real Word footnote parts (`footnotes.xml`).
//! - **Lists** use a marker glyph/number prefix, not native Word numbering.
//! - **Links/images** render their text/alt only; no hyperlink relation or embedded media.
//! - **Block quotes** recurse without an indent/border.

use docx_rs::{Docx, Paragraph, Run, RunFonts, Table, TableCell, TableRow};

use crate::ast::{Block, Inline};

/// Font used for inline code and code blocks.
const MONO_FONT: &str = "Consolas";

/// Build a complete [`Docx`] from a parsed document.
///
/// Footnote definitions are pulled out of the flow and appended under a "Notes" heading so
/// that reference order in the body is preserved regardless of where the definitions sat
/// in the source.
pub fn build(blocks: &[Block]) -> Docx {
    let mut docx = Docx::new();

    // First pass: emit the body, holding footnote definitions aside.
    let mut notes: Vec<&Block> = Vec::new();
    for block in blocks {
        if let Block::FootnoteDef { .. } = block {
            notes.push(block);
        } else {
            docx = write_block(docx, block);
        }
    }

    // Second pass: a trailing notes section, only if the document actually had any.
    if !notes.is_empty() {
        docx = docx.add_paragraph(heading_paragraph(2, &[Inline::Text("Notes".into())]));
        for note in notes {
            if let Block::FootnoteDef { label, content } = note {
                // Prefix the note's first paragraph with its label so references resolve
                // visually; deeper structure inside a note is rendered as-is.
                let marker = format!("[{label}] ");
                docx = write_note(docx, &marker, content);
            }
        }
    }

    docx
}

/// Emit one block into the document, returning the extended [`Docx`].
fn write_block(docx: Docx, block: &Block) -> Docx {
    match block {
        Block::Heading { level, content } => docx.add_paragraph(heading_paragraph(*level, content)),
        Block::Paragraph(inlines) => docx.add_paragraph(paragraph(inlines, false, false)),
        Block::CodeBlock { code, .. } => write_code_block(docx, code),
        Block::BlockQuote(inner) => write_blocks(docx, inner),
        Block::List {
            ordered,
            start,
            items,
        } => write_list(docx, *ordered, *start, items),
        Block::Table { headers, rows } => docx.add_table(table(headers, rows)),
        Block::ThematicBreak => {
            docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text("————————————————")))
        }
        // FootnoteDef is handled in `build`; if one reaches here, render it plainly.
        Block::FootnoteDef { label, content } => write_note(docx, &format!("[{label}] "), content),
    }
}

/// Emit a sequence of blocks (used for block-quote and note bodies).
fn write_blocks(mut docx: Docx, blocks: &[Block]) -> Docx {
    for block in blocks {
        docx = write_block(docx, block);
    }
    docx
}

/// Build a heading paragraph: bold, with a size that decreases as the level deepens.
///
/// We format inline rather than via a named Word style so headings look right even though
/// v0.1 does not yet ship a curated `styles.xml`.
fn heading_paragraph(level: u8, content: &[Inline]) -> Paragraph {
    let size = heading_half_points(level);
    let mut paragraph = Paragraph::new();
    // Headings are always bold; carry the level's size onto every run.
    for run in runs(content, true, false) {
        paragraph = paragraph.add_run(run.size(size));
    }
    paragraph
}

/// Half-point font size for a heading level (`size()` in `docx-rs` is in half-points, so
/// e.g. 40 → 20pt). H1 is largest; anything past H6 shares the H6 size.
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

/// Build a plain paragraph from inline content with the given base emphasis flags.
fn paragraph(inlines: &[Inline], bold: bool, italic: bool) -> Paragraph {
    let mut paragraph = Paragraph::new();
    for run in runs(inlines, bold, italic) {
        paragraph = paragraph.add_run(run);
    }
    paragraph
}

/// Flatten an inline tree into `docx-rs` runs, threading the accumulated bold/italic flags
/// down through nested emphasis.
fn runs(inlines: &[Inline], bold: bool, italic: bool) -> Vec<Run> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) => out.push(styled(t, bold, italic)),
            // Nesting just flips one flag and recurses; the leaves become runs.
            Inline::Emph(inner) => out.extend(runs(inner, bold, true)),
            Inline::Strong(inner) => out.extend(runs(inner, true, italic)),
            Inline::Code(t) => out.push(mono(t)),
            // v0.1: a link contributes only its (formatted) visible text.
            Inline::Link { text, .. } => out.extend(runs(text, bold, italic)),
            // A soft break is just whitespace between words.
            Inline::SoftBreak => out.push(styled(" ", bold, italic)),
            Inline::HardBreak => out.push(Run::new().add_break(docx_rs::BreakType::TextWrapping)),
            // Reference marker; real Word footnote parts are a follow-up (see module docs).
            Inline::FootnoteRef(label) => out.push(styled(&format!("[{label}]"), bold, italic)),
            Inline::Image(alt) => out.push(styled(&format!("[image: {alt}]"), bold, italic)),
        }
    }
    out
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

/// Emit a code block as one monospace paragraph per source line so line breaks survive.
fn write_code_block(mut docx: Docx, code: &str) -> Docx {
    // `lines()` drops a trailing newline; an empty block yields no paragraphs, which is fine.
    for line in code.lines() {
        docx = docx.add_paragraph(Paragraph::new().add_run(mono(line)));
    }
    docx
}

/// Emit a list. v0.1 prefixes each item with a bullet glyph or `N.` and renders the item's
/// first paragraph; a nested list inside an item recurses (getting its own markers).
fn write_list(mut docx: Docx, ordered: bool, start: u64, items: &[Vec<Block>]) -> Docx {
    for (index, item) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}.\t", start + index as u64)
        } else {
            "•\t".to_string()
        };

        // Lead with the marker, then the item's first paragraph of inline content.
        let mut paragraph = Paragraph::new().add_run(Run::new().add_text(marker));
        if let Some(Block::Paragraph(inlines)) =
            item.iter().find(|b| matches!(b, Block::Paragraph(_)))
        {
            for run in runs(inlines, false, false) {
                paragraph = paragraph.add_run(run);
            }
        }
        docx = docx.add_paragraph(paragraph);

        // Render nested lists that live inside this item.
        for block in item {
            if let Block::List { .. } = block {
                docx = write_block(docx, block);
            }
        }
    }
    docx
}

/// Emit a footnote definition as a marker-prefixed paragraph followed by any extra blocks.
fn write_note(mut docx: Docx, marker: &str, content: &[Block]) -> Docx {
    // Splice the marker onto the first paragraph so `[label]` sits inline with the text.
    if let Some(Block::Paragraph(inlines)) =
        content.iter().find(|b| matches!(b, Block::Paragraph(_)))
    {
        let mut paragraph = Paragraph::new().add_run(Run::new().add_text(marker));
        for run in runs(inlines, false, false) {
            paragraph = paragraph.add_run(run);
        }
        docx = docx.add_paragraph(paragraph);
    }
    // Any non-paragraph content in the note (nested list, code) is rendered after it.
    for block in content {
        if !matches!(block, Block::Paragraph(_)) {
            docx = write_block(docx, block);
        }
    }
    docx
}

/// Build a `docx-rs` [`Table`] from header + body cells.
fn table(headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> Table {
    let mut table_rows = Vec::new();

    // Header row is bold to read like a GFM table's header.
    let header_cells: Vec<TableCell> = headers
        .iter()
        .map(|cell| TableCell::new().add_paragraph(paragraph(cell, true, false)))
        .collect();
    table_rows.push(TableRow::new(header_cells));

    // Body rows render their cells plainly.
    for row in rows {
        let cells: Vec<TableCell> = row
            .iter()
            .map(|cell| TableCell::new().add_paragraph(paragraph(cell, false, false)))
            .collect();
        table_rows.push(TableRow::new(cells));
    }

    Table::new(table_rows)
}
