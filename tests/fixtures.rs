//! The shared fixtures, and proof that the reader understands every one of them.
//!
//! `tests/fixtures/` is the input set of the phase 2 ports (DOCX, PPTX + audit, twin +
//! PDF). What is in it:
//!
//! - `syntax.md` — every construction the reader knows, once: front matter, attributes,
//!   inline/figure/data-URI/remote images, links, footnotes and inline notes, inline and
//!   display math (and math in code spans), citations, an aligned and captioned table,
//!   task lists, nested lists, quotes, slide breaks, speaker notes, columns, a custom div,
//!   code with attributes, Mermaid, raw HTML.
//! - `deck.md` — a deck exercising PPTX layout selection: cover from front matter,
//!   `#` sections, `.bignumber`, `{layout="…"}`, columns, a statement, a quote, a lone
//!   figure, a table, a recap title, notes after content, a `.closing` slide.
//! - From the Python md2star's own tests and examples: `simple.md`, `metadata.md`,
//!   `mermaid.md`, `bibliography.md` (+ `refs.bib`), `images.md` (its logo pointed at
//!   our PNG), `math.md`, `comprehensive_document.md`, `comprehensive_presentation.md`,
//!   `table_zoo.md`, `footnotes_document.md`, `guide_complet_document_fr.md`,
//!   `branded_slides.md`, and `references.bib` (36 entries, the examples' master file).
//! - `images/` — `pixel-4x3.png`, `photo-8x6.jpg`, `diagram.svg` (200×100): tiny, with
//!   distinct sizes so a test can tell them apart by their dimensions.
//! - `templates/` — the Python md2star's bundled `template.docx` and `template.pptx`
//!   (BSD-3-Clause, same author) and its branded-deck example `Presentation1.pptx`.
//! - `python-reference/` — what md2star (Pandoc) produced for the same input, to compare
//!   element by element: `footnotes_document.{docx,pptx}` and `with_bib.pptx`, with
//!   their Markdown.

use std::fs;
use std::path::{Path, PathBuf};

use md2star_rs::ast::{Block, CitationMode, Inline};
use md2star_rs::reader::read;
use md2star_rs::resources::{ImageFormat, Resolver};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(name: &str) -> md2star_rs::Document {
    let path = fixtures().join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    read(&text)
}

/// Every block of a document, depth first (inside quotes, lists, divs, notes, footnotes).
fn all_blocks(blocks: &[Block]) -> Vec<&Block> {
    let mut out = Vec::new();
    for b in blocks {
        out.push(b);
        match b {
            Block::BlockQuote(inner)
            | Block::Notes(inner)
            | Block::Div { content: inner, .. }
            | Block::FootnoteDef { content: inner, .. } => out.extend(all_blocks(inner)),
            Block::List { items, .. } => {
                for item in items {
                    out.extend(all_blocks(&item.content));
                }
            }
            _ => {}
        }
    }
    out
}

/// Every inline of a document, depth first.
fn all_inlines(blocks: &[Block]) -> Vec<Inline> {
    fn walk(inlines: &[Inline], out: &mut Vec<Inline>) {
        for i in inlines {
            out.push(i.clone());
            match i {
                Inline::Emph(c)
                | Inline::Strong(c)
                | Inline::Strikeout(c)
                | Inline::Superscript(c)
                | Inline::Subscript(c)
                | Inline::InlineNote(c) => walk(c, out),
                Inline::Link { text, .. } => walk(text, out),
                Inline::Image(image) => walk(&image.alt, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for b in all_blocks(blocks) {
        match b {
            Block::Paragraph(p) | Block::Heading { content: p, .. } => walk(p, &mut out),
            Block::Figure(image) => out.push(Inline::Image(image.clone())),
            Block::Table {
                headers,
                rows,
                caption,
                ..
            } => {
                for cell in headers.iter().chain(rows.iter().flatten()) {
                    walk(cell, &mut out);
                }
                walk(caption, &mut out);
            }
            _ => {}
        }
    }
    out
}

#[test]
fn every_markdown_fixture_reads_without_warnings() {
    let mut count = 0;
    for entry in fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let doc = read(&fs::read_to_string(&path).unwrap());
        assert!(
            doc.warnings.is_empty(),
            "{}: {:?}",
            path.display(),
            doc.warnings
        );
        assert!(!doc.blocks.is_empty(), "{} read as empty", path.display());
        count += 1;
    }
    assert!(count >= 14, "fixtures went missing: only {count}");
}

#[test]
fn syntax_fixture_metadata() {
    let doc = read_fixture("syntax.md");
    let m = &doc.meta;
    assert_eq!(m.title.as_deref(), Some("Every construction md2star reads"));
    assert_eq!(m.authors, vec!["Ada Lovelace", "Grace Hopper"]);
    assert_eq!(m.bibliography, vec!["refs.bib"]);
    assert_eq!(m.toc, Some(true));
    assert!(m.raw.contains_key("custom"));
}

#[test]
fn syntax_fixture_covers_every_block_kind() {
    let doc = read_fixture("syntax.md");
    let blocks = all_blocks(&doc.blocks);
    let has = |pred: &dyn Fn(&Block) -> bool| blocks.iter().any(|b| pred(b));
    assert!(has(
        &|b| matches!(b, Block::Heading { attr, .. } if attr.id.as_deref() == Some("intro") && attr.has_class("unnumbered"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Heading { attr, .. } if attr.get("layout") == Some("Big Number"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Figure(i) if i.attr.id.as_deref() == Some("fig:photo"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Figure(i) if i.src.ends_with(".svg"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Table { caption, attr, aligns, .. }
        if !caption.is_empty() && attr.id.as_deref() == Some("tbl:aligned") && aligns.len() == 4)
    ));
    assert!(has(
        &|b| matches!(b, Block::List { items, .. } if items.iter().any(|i| i.task == Some(true)))
    ));
    assert!(has(&|b| matches!(
        b,
        Block::List {
            ordered: true,
            start: 3,
            ..
        }
    )));
    assert!(has(&|b| matches!(b, Block::BlockQuote(_))));
    assert!(has(&|b| matches!(b, Block::Notes(_))));
    assert!(has(
        &|b| matches!(b, Block::Div { attr, .. } if attr.has_class("columns"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Div { attr, .. } if attr.get("width") == Some("40%"))
    ));
    assert!(has(
        &|b| matches!(b, Block::Div { attr, .. } if attr.has_class("warning"))
    ));
    assert!(has(
        &|b| matches!(b, Block::CodeBlock { lang: Some(l), attr, .. } if l == "python" && attr.id.as_deref() == Some("listing"))
    ));
    assert!(has(&|b| matches!(b, Block::Mermaid { .. })));
    assert!(has(
        &|b| matches!(b, Block::FootnoteDef { content, .. } if content.len() == 2)
    ));
    assert!(has(
        &|b| matches!(b, Block::RawHtml(h) if h.contains("HTML comment"))
    ));
    let breaks = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::ThematicBreak))
        .count();
    assert_eq!(breaks, 3, "slide separators");
}

#[test]
fn syntax_fixture_covers_every_inline_kind() {
    let doc = read_fixture("syntax.md");
    let inlines = all_inlines(&doc.blocks);
    let has = |pred: &dyn Fn(&Inline) -> bool| inlines.iter().any(pred);
    assert!(has(&|i| matches!(i, Inline::Strong(_))));
    assert!(has(&|i| matches!(i, Inline::Emph(_))));
    assert!(has(&|i| matches!(i, Inline::Strikeout(_))));
    assert!(has(&|i| matches!(i, Inline::Subscript(_))));
    assert!(has(&|i| matches!(i, Inline::Superscript(_))));
    assert!(has(&|i| matches!(i, Inline::Code(_))));
    assert!(has(&|i| matches!(i, Inline::HardBreak)));
    assert!(has(
        &|i| matches!(i, Inline::Link { attr, .. } if attr.has_class("external"))
    ));
    assert!(has(
        &|i| matches!(i, Inline::Image(img) if img.src.starts_with("data:image/png"))
    ));
    assert!(has(
        &|i| matches!(i, Inline::Image(img) if img.attr.get("width") == Some("1in"))
    ));
    assert!(has(
        &|i| matches!(i, Inline::Image(img) if img.src.starts_with("https://"))
    ));
    assert!(has(&|i| matches!(i, Inline::FootnoteRef(_))));
    assert!(has(&|i| matches!(i, Inline::InlineNote(_))));
    assert!(has(&|i| matches!(i, Inline::Math { display: false, .. })));
    assert!(has(
        &|i| matches!(i, Inline::Math { display: true, tex } if tex.contains("\\sqrt{\\pi}"))
    ));
    assert!(has(
        &|i| matches!(i, Inline::Math { tex, .. } if tex == "\\frac{a}{b}")
    ));
    assert!(has(
        &|i| matches!(i, Inline::Cite(c) if c.len() == 2 && c[0].suffix == "p. 3" && c[1].prefix == "see also")
    ));
    assert!(has(
        &|i| matches!(i, Inline::Cite(c) if c[0].mode == CitationMode::AuthorInText && c[0].suffix == "chap. 2")
    ));
    assert!(has(
        &|i| matches!(i, Inline::Cite(c) if c[0].mode == CitationMode::SuppressAuthor)
    ));
    // The e-mail address stayed text.
    assert!(has(
        &|i| matches!(i, Inline::Text(t) if t.contains("someone@example.org"))
    ));
}

#[test]
fn every_local_image_of_the_syntax_fixture_resolves() {
    let doc = read_fixture("syntax.md");
    let resolver = Resolver::new(Some(fixtures()));
    let mut formats = Vec::new();
    for inline in all_inlines(&doc.blocks) {
        if let Inline::Image(image) = inline {
            match resolver.load_image(&image.src) {
                Ok(loaded) => formats.push(loaded.info.format),
                // The one remote reference is refused: no `net` feature, no opt-in.
                Err(md2star_rs::Error::Resource { src, .. }) => {
                    assert!(src.starts_with("https://"), "{src} failed to load")
                }
                Err(other) => panic!("{other}"),
            }
        }
    }
    for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Svg] {
        assert!(formats.contains(&f), "{f:?} not exercised: {formats:?}");
    }
}

#[test]
fn deck_fixture_has_the_hints_layout_selection_needs() {
    let doc = read_fixture("deck.md");
    assert_eq!(doc.meta.title.as_deref(), Some("Q3 review"));
    let blocks = all_blocks(&doc.blocks);
    let classes: Vec<String> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading { attr, .. } => Some(attr.classes.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(classes.contains(&"bignumber".to_string()));
    assert!(classes.contains(&"closing".to_string()));
    assert!(blocks.iter().any(
        |b| matches!(b, Block::Heading { attr, .. } if attr.get("layout") == Some("Two Content"))
    ));
    assert_eq!(
        blocks
            .iter()
            .filter(|b| matches!(b, Block::Notes(_)))
            .count(),
        2
    );
}

#[test]
fn templates_and_reference_outputs_are_office_zips() {
    for rel in [
        "templates/template.docx",
        "templates/template.pptx",
        "templates/Presentation1.pptx",
        "python-reference/footnotes_document.docx",
        "python-reference/footnotes_document.pptx",
        "python-reference/with_bib.pptx",
    ] {
        let bytes = fs::read(fixtures().join(rel)).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04", "{rel} is not a zip");
    }
    assert!(Path::new(&fixtures().join("refs.bib")).exists());
}

#[test]
fn every_fixture_converts_to_docx() {
    for entry in fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let markdown = fs::read_to_string(&path).unwrap();
        let out = md2star_rs::convert(
            &markdown,
            md2star_rs::Format::Docx,
            &md2star_rs::Options::default(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&out.bytes[..2], b"PK");
    }
}
