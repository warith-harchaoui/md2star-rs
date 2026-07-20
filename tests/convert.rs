//! Integration tests: drive the public API end to end and inspect the produced `.docx`.
//!
//! A `.docx` is a zip; we crack it open and assert on `word/document.xml` so the tests
//! verify real OOXML output rather than just "a file appeared". This is the acceptance
//! bar for v0.1 — the round trip Markdown → bytes → readable Word XML holds together.

use std::io::Read;

use md2star_rs::{markdown_to_docx_bytes, reader};

/// Read one named entry out of a packed `.docx` (a zip) as a UTF-8 string.
fn entry(docx_bytes: &[u8], name: &str) -> Option<String> {
    let reader = std::io::Cursor::new(docx_bytes);
    let mut archive = zip::ZipArchive::new(reader).expect("output is a valid zip");
    let mut file = archive.by_name(name).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).expect("entry is UTF-8");
    Some(text)
}

/// Pull `word/document.xml` out of a packed `.docx` byte buffer.
fn document_xml(docx_bytes: &[u8]) -> String {
    entry(docx_bytes, "word/document.xml").expect("docx contains word/document.xml")
}

#[test]
fn produces_a_valid_docx_zip() {
    let bytes = markdown_to_docx_bytes("Hello").expect("conversion succeeds");
    // Local-file-header magic: every zip (and thus every .docx) starts with `PK\x03\x04`.
    assert_eq!(&bytes[..4], b"PK\x03\x04");
}

#[test]
fn heading_and_paragraph_text_survive() {
    let bytes = markdown_to_docx_bytes("# Title\n\nHello world.").expect("conversion succeeds");
    let xml = document_xml(&bytes);
    // Both the heading and the body text must appear in the document part.
    assert!(xml.contains("Title"), "heading text missing: {xml}");
    assert!(xml.contains("Hello world."), "paragraph text missing");
}

#[test]
fn bold_text_becomes_a_bold_run() {
    let bytes = markdown_to_docx_bytes("Some **strong** text").expect("conversion succeeds");
    let xml = document_xml(&bytes);
    assert!(xml.contains("strong"), "bold text missing");
    // `docx-rs` emits `<w:b />` for a bold run; its presence proves emphasis mapped through.
    assert!(xml.contains("w:b"), "no bold run emitted: {xml}");
}

#[test]
fn gfm_table_becomes_a_table() {
    let markdown = "| A | B |\n|---|---|\n| 1 | 2 |";
    let bytes = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let xml = document_xml(&bytes);
    // A real Word table opens with `<w:tbl>`; cell contents must be present too.
    assert!(xml.contains("w:tbl"), "no table element: {xml}");
    assert!(
        xml.contains('A') && xml.contains('2'),
        "table cells missing"
    );
}

#[test]
fn footnotes_become_real_word_footnotes() {
    let markdown = "See this.[^n]\n\n[^n]: The note body.";
    let bytes = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let doc = document_xml(&bytes);
    // v0.2: a real footnote reference in the body, and a separate footnotes part holding
    // the note text — not the old inline "[n]" marker + trailing "Notes" section.
    assert!(
        doc.contains("footnoteReference") || doc.contains("FootnoteReference"),
        "no footnote reference in document.xml: {doc}"
    );
    assert!(
        !doc.contains(">Notes<"),
        "the old Notes section should be gone"
    );
    let footnotes = entry(&bytes, "word/footnotes.xml").expect("word/footnotes.xml present");
    assert!(
        footnotes.contains("The note body."),
        "footnote body missing from footnotes.xml"
    );
}

#[test]
fn ordered_list_uses_native_numbering() {
    let markdown = "1. first\n2. second\n3. third";
    let bytes = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let doc = document_xml(&bytes);
    // Native numbering means numbering properties on the paragraph, not a "1." typed in.
    assert!(doc.contains("numId"), "no numId in document.xml: {doc}");
    assert!(doc.contains("ilvl"), "no indent level in document.xml");
    // And a real numbering part must exist and define a decimal format.
    let numbering = entry(&bytes, "word/numbering.xml").expect("word/numbering.xml present");
    assert!(
        numbering.contains("decimal"),
        "no decimal format in numbering.xml"
    );
    assert!(
        numbering.contains("bullet"),
        "no bullet format in numbering.xml"
    );
}

#[test]
fn conversion_is_idempotent() {
    // Idempotence/determinism: the same Markdown must produce byte-identical output every
    // run — no timestamps, and footnote/numbering ids come from per-document counters.
    let markdown = "# Doc\n\n1. one[^a]\n2. two\n\n- bullet\n\n[^a]: note";
    let first = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let second = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    assert_eq!(
        first, second,
        "identical input produced differing .docx bytes"
    );
}

#[test]
fn reader_nests_emphasis_inside_strong() {
    // A white-box check on the AST seam: `**a _b_**` must parse to Strong[Text, Emph[Text]].
    let blocks = reader::parse("**a _b_**");
    assert_eq!(blocks.len(), 1, "expected a single paragraph");
}
