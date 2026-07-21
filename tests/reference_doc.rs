//! Integration tests for the `--reference-doc` feature (Pandoc parity).
//!
//! Each test builds a *template* `.docx` in memory with `docx-rs` directly — a specific page
//! size and/or named styles — then converts Markdown against it and cracks open the produced
//! `.docx` to assert the template's styling came through. Generating the template in-test (no
//! binary fixture in the repo) keeps every assertion self-contained and readable: the exact
//! styling being tested sits right next to the assertion.

use std::io::{Cursor, Read};

use docx_rs::{Docx, Style, StyleType};
use md2star_rs::{markdown_to_docx_bytes_with_reference, Error};

/// Pack a `docx-rs` [`Docx`] into raw `.docx` bytes — the in-test equivalent of the library's
/// own packing step, used to synthesize template documents.
fn pack(docx: Docx) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());
    docx.build().pack(&mut buffer).expect("template packs");
    buffer.into_inner()
}

/// Read one named entry out of a packed `.docx` (a zip) as a UTF-8 string.
fn entry(docx_bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(docx_bytes)).expect("output is a valid zip");
    let mut file = archive.by_name(name).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).expect("entry is UTF-8");
    Some(text)
}

/// Pull `word/document.xml` out of a packed `.docx` byte buffer.
fn document_xml(docx_bytes: &[u8]) -> String {
    entry(docx_bytes, "word/document.xml").expect("docx contains word/document.xml")
}

/// A template that declares a paragraph style with the given id (e.g. `"Heading1"`, `"Quote"`).
///
/// The style is given a distinctive bold + large size so that, if the writer wrongly emitted
/// inline formatting instead of referencing the style, the difference would be observable.
fn template_with_style(style_id: &str) -> Vec<u8> {
    let style = Style::new(style_id, StyleType::Paragraph)
        .name(style_id)
        .bold()
        .size(48);
    pack(Docx::new().add_style(style))
}

#[test]
fn reference_template_page_size_is_inherited() {
    // US-Letter template (12240 x 15840 twips) — deliberately different from docx-rs's default
    // A4 (11906 x 16838) so inheritance is unambiguous: the output must carry Letter, not A4.
    let template = pack(Docx::new().page_size(12240, 15840));
    let bytes =
        markdown_to_docx_bytes_with_reference("# Title\n\nBody.", &template).expect("converts");
    let doc = document_xml(&bytes);
    // The section's page size comes from the template's section_property, which we keep.
    assert!(
        doc.contains("w:w=\"12240\"") && doc.contains("w:h=\"15840\""),
        "output did not inherit the template's page size: {doc}"
    );
    // And the actual Markdown content is still present.
    assert!(
        doc.contains("Title") && doc.contains("Body."),
        "content missing"
    );
}

#[test]
fn heading_uses_template_named_style() {
    // Template defines a Heading1 style → the heading paragraph must *reference* it, not carry
    // our inline heading size. That reference is the whole point of a reference document.
    let template = template_with_style("Heading1");
    let bytes = markdown_to_docx_bytes_with_reference("# Title", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        doc.contains("w:val=\"Heading1\""),
        "heading did not use the template's Heading1 style: {doc}"
    );
}

#[test]
fn level_six_heading_uses_heading6_style() {
    // Level 6 is the deepest heading CommonMark emits (`#######` is literal text, not a level-7
    // heading), and it is also the deepest built-in Word style — so `######` must map to Heading6.
    // This is the upper end of the `min(MAX_HEADING_STYLE)` clamp exercised through real input.
    let template = template_with_style("Heading6");
    let bytes = markdown_to_docx_bytes_with_reference("###### Deep", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        doc.contains("w:val=\"Heading6\""),
        "level-6 heading did not use the Heading6 style: {doc}"
    );
}

#[test]
fn heading_without_template_style_falls_back_to_inline() {
    // Template has NO Heading1 style → we must fall back to inline formatting (a real font
    // size on the run), never emit a dangling reference to a style the template lacks.
    let template = pack(Docx::new()); // no styles at all
    let bytes = markdown_to_docx_bytes_with_reference("# Title", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        !doc.contains("w:val=\"Heading1\""),
        "referenced a Heading1 style the template never defined: {doc}"
    );
    // Inline fallback carries an explicit run size (`<w:sz .../>`).
    assert!(
        doc.contains("w:sz"),
        "fallback heading lost its inline size: {doc}"
    );
}

#[test]
fn blockquote_uses_quote_style_when_present() {
    // Template defines a Quote style → a block quote's paragraph is styled with it.
    let template = template_with_style("Quote");
    let bytes =
        markdown_to_docx_bytes_with_reference("> quoted line", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        doc.contains("w:val=\"Quote\""),
        "block quote did not use the template's Quote style: {doc}"
    );
    assert!(doc.contains("quoted line"), "quote text missing");
}

#[test]
fn list_numbering_does_not_collide_with_template() {
    // A template that already carries a Heading1 style plus (implicitly) its own numbering part
    // must not clash with the numbering ids we add for the Markdown's ordered list. The output
    // must still render native numbering (a `numId`) and remain a valid docx.
    let template = template_with_style("Heading1");
    let bytes =
        markdown_to_docx_bytes_with_reference("1. one\n2. two", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        doc.contains("numId"),
        "ordered list lost its numbering: {doc}"
    );
    // The numbering part must exist and define our decimal format.
    let numbering = entry(&bytes, "word/numbering.xml").expect("numbering.xml present");
    assert!(
        numbering.contains("decimal"),
        "no decimal numbering emitted"
    );
}

#[test]
fn reference_conversion_is_idempotent() {
    // The determinism guarantee holds on the reference path too: same Markdown + same template
    // → byte-identical output (paragraph/footnote/numbering ids all come from per-doc counters).
    let template = template_with_style("Heading1");
    let markdown = "# Title\n\n1. one[^a]\n\n[^a]: note";
    let first = markdown_to_docx_bytes_with_reference(markdown, &template).expect("converts");
    let second = markdown_to_docx_bytes_with_reference(markdown, &template).expect("converts");
    assert_eq!(
        first, second,
        "reference conversion is not byte-deterministic"
    );
}

#[test]
fn invalid_reference_document_is_a_template_error() {
    // Garbage bytes are not a readable `.docx`; the caller gets a typed Template error (not a
    // panic, and not misclassified as an I/O error).
    let err = markdown_to_docx_bytes_with_reference("# Title", b"not a docx at all")
        .expect_err("must reject a non-docx template");
    assert!(
        matches!(err, Error::Template(_)),
        "expected Error::Template, got {err:?}"
    );
}
