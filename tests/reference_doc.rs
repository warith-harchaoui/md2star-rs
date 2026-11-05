//! The v0.4 entry point `markdown_to_docx_bytes_with_reference` (Pandoc's
//! `--reference-doc`), now backed by the full DOCX writer.
//!
//! Each test writes a small reference `.docx` by hand (`docx_common`): a page size and a
//! few named styles, next to the assertion that depends on them. The fuller contract of
//! the reference path (every part kept, numbering merged) is in `docx_reference.rs`.

mod docx_common;

use std::io::{Cursor, Read};

use docx_common::{reference, word_document};
use md2star_rs::{markdown_to_docx_bytes, markdown_to_docx_bytes_with_reference, Error};

/// Read one named entry out of a packed `.docx` (a zip) as a UTF-8 string.
fn entry(docx_bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(docx_bytes)).expect("output is a valid zip");
    let mut file = archive.by_name(name).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).expect("entry is UTF-8");
    Some(text)
}

fn document_xml(docx_bytes: &[u8]) -> String {
    entry(docx_bytes, "word/document.xml").expect("docx contains word/document.xml")
}

fn styles_xml(docx_bytes: &[u8]) -> String {
    entry(docx_bytes, "word/styles.xml").expect("docx contains word/styles.xml")
}

#[test]
fn reference_template_page_size_is_inherited() {
    // US Letter, unlike the A4 of the bundled default: inheritance is unambiguous.
    let template = word_document(&[], (12240, 15840), "");
    let bytes =
        markdown_to_docx_bytes_with_reference("# Title\n\nBody.", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(
        doc.contains("w:w=\"12240\"") && doc.contains("w:h=\"15840\""),
        "output did not inherit the template's page size: {doc}"
    );
    assert!(doc.contains("Body."), "content missing");
}

#[test]
fn heading_uses_the_templates_own_style_definition() {
    let template = reference(&[("Heading1", "heading 1")]);
    let bytes =
        markdown_to_docx_bytes_with_reference("# Title\n\n## Sub", &template).expect("converts");
    // The first `#` is the title (md2star's rule): use a second level-1 heading.
    let bytes2 =
        markdown_to_docx_bytes_with_reference("# Title\n\n# Part", &template).expect("converts");
    assert!(document_xml(&bytes2).contains("w:val=\"Heading1\""));
    let styles = styles_xml(&bytes);
    let heading1 = styles
        .split("<w:style ")
        .find(|s| s.contains("w:styleId=\"Heading1\""))
        .expect("Heading1 kept");
    assert!(
        heading1.contains("<w:sz w:val=\"48\"/>"),
        "the template's Heading1 was replaced: {heading1}"
    );
}

#[test]
fn level_six_heading_uses_heading6_style() {
    let template = reference(&[("Heading6", "heading 6")]);
    let bytes =
        markdown_to_docx_bytes_with_reference("# T\n\n###### Deep", &template).expect("converts");
    assert!(document_xml(&bytes).contains("w:val=\"Heading6\""));
}

#[test]
fn a_style_the_template_lacks_is_added_not_left_dangling() {
    let template = reference(&[]); // no styles at all
    let bytes = markdown_to_docx_bytes_with_reference(
        "# T\n\n# Part\n\n> quoted\n\n```\nx\n```",
        &template,
    )
    .expect("converts");
    let doc = document_xml(&bytes);
    let styles = styles_xml(&bytes);
    for id in ["Heading1", "BlockText", "SourceCode"] {
        assert!(
            doc.contains(&format!("w:val=\"{id}\"")),
            "{id} not used: {doc}"
        );
        assert!(
            styles.contains(&format!("w:styleId=\"{id}\"")),
            "{id} used but not defined"
        );
    }
}

#[test]
fn a_style_found_by_name_keeps_the_templates_id() {
    // Word localises style ids but not the built-in names: "heading 1" under another id.
    let template = reference(&[("Titre1", "heading 1")]);
    let bytes =
        markdown_to_docx_bytes_with_reference("# T\n\n# Partie", &template).expect("converts");
    assert!(document_xml(&bytes).contains("w:val=\"Titre1\""));
}

#[test]
fn ordered_lists_get_native_numbering() {
    let template = reference(&[("Heading1", "heading 1")]);
    let bytes =
        markdown_to_docx_bytes_with_reference("1. one\n2. two", &template).expect("converts");
    assert!(document_xml(&bytes).contains("numId"));
    let numbering = entry(&bytes, "word/numbering.xml").expect("numbering.xml present");
    assert!(
        numbering.contains("decimal"),
        "no decimal numbering emitted"
    );
}

#[test]
fn the_templates_body_is_dropped() {
    let template = word_document(
        &[],
        (11906, 16838),
        "<w:p><w:r><w:t>TEMPLATE BODY</w:t></w:r></w:p>",
    );
    let bytes = markdown_to_docx_bytes_with_reference("Mine.", &template).expect("converts");
    let doc = document_xml(&bytes);
    assert!(!doc.contains("TEMPLATE BODY") && doc.contains("Mine."));
}

#[test]
fn without_a_reference_md2stars_template_is_the_default() {
    let bundled = md2star_rs::templates::DEFAULT_DOCX;
    let plain = markdown_to_docx_bytes("# T\n\nBody.").expect("converts");
    let explicit =
        markdown_to_docx_bytes_with_reference("# T\n\nBody.", bundled).expect("converts");
    assert_eq!(plain, explicit);
}

#[test]
fn reference_conversion_is_idempotent() {
    let template = reference(&[("Heading1", "heading 1")]);
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
    let err = markdown_to_docx_bytes_with_reference("# Title", b"not a docx at all")
        .expect_err("must reject a non-docx template");
    assert!(
        matches!(err, Error::Template(_)),
        "expected Error::Template, got {err:?}"
    );
}
