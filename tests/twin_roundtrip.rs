//! Round trips: Markdown → md2star-rs (DOCX / PPTX) → `twin` → Markdown, compared with
//! the original after normalisation.
//!
//! "Normalised" means both sides are parsed by the same reader and projected onto what
//! a document can carry: the kind of each block, its plain text with whitespace
//! collapsed, list depth and type, table cells, code, footnote bodies. What the writers
//! legitimately change is projected away — formula delimiters (a writer without OMML
//! keeps the LaTeX as text), heading *depth* for decks (a deck has slides, not levels),
//! footnote labels (Word renumbers them).

use std::io::Cursor;
use std::path::PathBuf;

use docx_rs::{Docx, Style, StyleType};

use md2star_rs::ast::{plain_text, Block, Inline};
use md2star_rs::twin::{self, SourceKind, TwinOptions};
use md2star_rs::{reader, Format, Options};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn norm(s: &str) -> String {
    // Pandoc's smart quotes are the same characters to a reader.
    let s: String = s
        .chars()
        // Formula delimiters and grouping braces: `x^{2}` and `x^2` are one formula.
        .filter(|c| !matches!(c, '$' | '{' | '}'))
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            other => other,
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text(inlines: &[Inline]) -> String {
    norm(&plain_text(inlines))
}

/// Project a document onto comparable lines. `deck` drops heading levels.
fn project(blocks: &[Block], deck: bool, depth: usize, out: &mut Vec<String>) {
    for b in blocks {
        match b {
            Block::Heading { level, content, .. } => {
                if deck {
                    out.push(format!("H {}", text(content)));
                } else {
                    out.push(format!("H{level} {}", text(content)));
                }
            }
            Block::Paragraph(inl) => {
                let t = text(inl);
                if !t.is_empty() {
                    out.push(format!("P {t}"));
                }
            }
            Block::List { ordered, items, .. } => {
                for item in items {
                    let first = item
                        .content
                        .iter()
                        .find_map(|c| match c {
                            Block::Paragraph(i) => Some(text(i)),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let task = match item.task {
                        Some(true) => "[x] ",
                        Some(false) => "[ ] ",
                        None => "",
                    };
                    out.push(format!(
                        "L{depth}{} {task}{first}",
                        if *ordered { "o" } else { "u" }
                    ));
                    let nested: Vec<Block> = item
                        .content
                        .iter()
                        .filter(|c| !matches!(c, Block::Paragraph(_)))
                        .cloned()
                        .collect();
                    project(&nested, deck, depth + 1, out);
                }
            }
            Block::Table { headers, rows, .. } => {
                let mut cells: Vec<String> = headers.iter().map(|c| text(c)).collect();
                for r in rows {
                    cells.extend(r.iter().map(|c| text(c)));
                }
                out.push(format!("T {}", cells.join(" | ")));
            }
            Block::CodeBlock { code, .. } | Block::Mermaid { code, .. } => {
                out.push(format!("C {}", code.trim_end()));
            }
            Block::BlockQuote(inner) => {
                let mut q = Vec::new();
                project(inner, deck, depth, &mut q);
                out.extend(q.into_iter().map(|l| format!("Q {l}")));
            }
            Block::Div { content, .. } => project(content, deck, depth, out),
            Block::FootnoteDef { content, .. } => {
                let mut n = Vec::new();
                project(content, deck, depth, &mut n);
                out.push(format!("N {}", n.join(" / ")));
            }
            Block::Figure(img) => out.push(format!("F {}", norm(&img.alt_text()))),
            Block::ThematicBreak | Block::Notes(_) | Block::RawHtml(_) => {}
        }
    }
}

/// Inline notes (`^[…]`) become ordinary footnotes in Word and in a deck.
fn inline_notes(blocks: &[Block], out: &mut Vec<String>) {
    fn walk(inlines: &[Inline], out: &mut Vec<String>) {
        for i in inlines {
            match i {
                Inline::InlineNote(n) => out.push(format!("N P {}", text(n))),
                Inline::Emph(x)
                | Inline::Strong(x)
                | Inline::Strikeout(x)
                | Inline::Superscript(x)
                | Inline::Subscript(x) => walk(x, out),
                Inline::Link { text, .. } => walk(text, out),
                _ => {}
            }
        }
    }
    for b in blocks {
        match b {
            Block::Paragraph(i) | Block::Heading { content: i, .. } => walk(i, out),
            Block::List { items, .. } => {
                for item in items {
                    inline_notes(&item.content, out);
                }
            }
            Block::BlockQuote(inner) | Block::Div { content: inner, .. } => {
                inline_notes(inner, out)
            }
            _ => {}
        }
    }
}

fn lines(markdown: &str, deck: bool) -> Vec<String> {
    let doc = reader::read(markdown);
    let mut out = Vec::new();
    project(&doc.blocks, deck, 0, &mut out);
    inline_notes(&doc.blocks, &mut out);
    // Footnote bodies are compared as a set: Word orders them by first reference.
    let (mut notes, mut rest): (Vec<String>, Vec<String>) =
        out.into_iter().partition(|l| l.starts_with("N "));
    notes.sort();
    rest.extend(notes);
    rest
}

fn assert_same(original: &str, twin: &str, deck: bool) {
    let a = lines(original, deck);
    let b = lines(twin, deck);
    if a != b {
        let mut report = String::new();
        for i in 0..a.len().max(b.len()) {
            let l = a.get(i).map(String::as_str).unwrap_or("∅");
            let r = b.get(i).map(String::as_str).unwrap_or("∅");
            let mark = if l == r { "  " } else { "≠ " };
            report.push_str(&format!("{mark}{l}\n  {r}\n"));
        }
        panic!("round trip differs:\n{report}\n--- twin ---\n{twin}");
    }
}

/// A reference document that declares the named styles a Word user would use, so the
/// writer emits headings, quotes and code through them (built in memory: the twin must
/// not depend on one particular template file).
fn template() -> Vec<u8> {
    let mut docx = Docx::new();
    for id in ["Heading1", "Heading2", "Heading3", "Quote", "SourceCode"] {
        let name = match id {
            "Heading1" => "heading 1",
            "Heading2" => "heading 2",
            "Heading3" => "heading 3",
            "SourceCode" => "Source Code",
            other => other,
        };
        docx = docx.add_style(Style::new(id, StyleType::Paragraph).name(name));
    }
    let mut buffer = Cursor::new(Vec::new());
    docx.build().pack(&mut buffer).expect("template packs");
    buffer.into_inner()
}

fn docx_options() -> Options {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o.reference_doc = Some(template());
    o
}

// A front-matter title: without one, md2star's rule (kept by the DOCX writer) turns the
// first `#` heading into the document title, and the twin gives it back as `title:`.
const DOCX_SAMPLE: &str = "\
---
title: Round trip
---

# Results

A paragraph with **bold**, *italic*, `code`, ~~struck~~, H~2~O and 2^10^, plus a
[link](https://deraison.ai) and a note.[^a]

## Lists

- one
- two
  1. nested first
  2. nested second
- three

1. alpha
2. beta

- [x] done
- [ ] open

## Table

| Name | Score |
|------|------:|
| Ada  | 10    |
| Alan | 9     |

> A quotation
> that spans lines.

```
fn main() {
    println!(\"hi\");
}
```

Text with $x^2$ inline and an escaped \\*star\\* and snake_case.

[^a]: The note body.
";

#[test]
fn docx_round_trip_with_the_reference_template() {
    let options = docx_options();
    let out = md2star_rs::convert(DOCX_SAMPLE, Format::Docx, &options).unwrap();
    let twin = twin::to_markdown(&out.bytes, SourceKind::Docx, &TwinOptions::default()).unwrap();
    assert_same(DOCX_SAMPLE, &twin.markdown, false);
}

#[test]
fn docx_twin_is_a_fixed_point() {
    // g(g(x)) == g(x): twinning the twin's own DOCX gives the same Markdown back.
    let options = docx_options();
    let once = {
        let out = md2star_rs::convert(DOCX_SAMPLE, Format::Docx, &options).unwrap();
        twin::to_markdown(&out.bytes, SourceKind::Docx, &TwinOptions::default())
            .unwrap()
            .markdown
    };
    let twice = {
        let out = md2star_rs::convert(&once, Format::Docx, &options).unwrap();
        twin::to_markdown(&out.bytes, SourceKind::Docx, &TwinOptions::default())
            .unwrap()
            .markdown
    };
    assert_eq!(once, twice);
}

#[test]
fn python_reference_docx_round_trips_to_its_source() {
    let md =
        std::fs::read_to_string(fixtures().join("python-reference/footnotes_document.md")).unwrap();
    let docx = std::fs::read(fixtures().join("python-reference/footnotes_document.docx")).unwrap();
    let twin = twin::to_markdown(&docx, SourceKind::Docx, &TwinOptions::default()).unwrap();
    // md2star (Python) lifts the first `#` into the Title; the twin gives it back as the
    // front-matter title, so compare the title separately and the bodies without it.
    let doc = reader::read(&twin.markdown);
    assert_eq!(doc.meta.title.as_deref(), Some("Footnotes Demonstration"));
    let source_body = md.replacen("# Footnotes Demonstration\n", "", 1);
    assert_same(&source_body, &twin.markdown, false);
}

#[test]
fn python_reference_pptx_round_trips_to_its_source() {
    let md =
        std::fs::read_to_string(fixtures().join("python-reference/footnotes_document.md")).unwrap();
    let pptx = std::fs::read(fixtures().join("python-reference/footnotes_document.pptx")).unwrap();
    let twin = twin::to_markdown(&pptx, SourceKind::Pptx, &TwinOptions::default()).unwrap();
    let doc = reader::read(&twin.markdown);
    assert_eq!(doc.meta.title.as_deref(), Some("Footnotes Demonstration"));
    // The deck's second slide carries the author and the date md2star added; the rest is
    // the source, slide for section.
    let twin_body = twin
        .markdown
        .replacen("Richard Feynman\n\nWednesday, July 15, 2026\n", "", 1);
    let source_body = md.replacen("# Footnotes Demonstration\n", "", 1);
    assert_same(&source_body, &twin_body, true);
}

#[test]
fn docx_round_trip_with_the_default_styles() {
    // No reference document: the writer's own styles carry the structure.
    let mut options = Options::default();
    options.base_dir = Some(fixtures());
    let out = md2star_rs::convert(DOCX_SAMPLE, Format::Docx, &options).unwrap();
    let twin = twin::to_markdown(&out.bytes, SourceKind::Docx, &TwinOptions::default()).unwrap();
    assert_same(DOCX_SAMPLE, &twin.markdown, false);
    assert_eq!(
        reader::read(&twin.markdown).meta.title.as_deref(),
        Some("Round trip")
    );
}
