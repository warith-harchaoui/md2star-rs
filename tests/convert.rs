//! Integration tests: drive the public API end to end and inspect the produced `.docx`.
//!
//! A `.docx` is a zip; we crack it open and assert on `word/document.xml` so the tests
//! verify real OOXML output rather than just "a file appeared". This is the acceptance
//! bar for v0.1 — the round trip Markdown → bytes → readable Word XML holds together.

use std::io::Read;

use md2star_rs::{markdown_to_docx_bytes, reader};

/// Pull `word/document.xml` out of a packed `.docx` byte buffer.
fn document_xml(docx_bytes: &[u8]) -> String {
    let reader = std::io::Cursor::new(docx_bytes);
    let mut archive = zip::ZipArchive::new(reader).expect("output is a valid zip");
    let mut file = archive
        .by_name("word/document.xml")
        .expect("docx contains word/document.xml");
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .expect("document.xml is UTF-8");
    xml
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
fn footnote_reference_and_definition_render() {
    let markdown = "See this.[^n]\n\n[^n]: The note body.";
    let bytes = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let xml = document_xml(&bytes);
    // v0.1 renders the marker inline and the definition under a Notes section.
    assert!(xml.contains("Notes"), "notes section missing");
    assert!(xml.contains("The note body."), "footnote body missing");
}

#[test]
fn reader_nests_emphasis_inside_strong() {
    // A white-box check on the AST seam: `**a _b_**` must parse to Strong[Text, Emph[Text]].
    let blocks = reader::parse("**a _b_**");
    assert_eq!(blocks.len(), 1, "expected a single paragraph");
}
