//! Integration tests for the PPTX backend (`md2pptx`).
//!
//! A `.pptx` is a zip of PresentationML parts; we crack it open and read the slide text out of
//! each `ppt/slides/slideN.xml` (text lives in `<a:t>…</a:t>` runs) to assert that Markdown mapped
//! onto slides the way the writer promises — one slide per `#`, body flattened to bullets.

use std::io::{Cursor, Read};

use md2star_rs::markdown_to_pptx_bytes;

/// The concatenated XML of every `ppt/slides/slideN.xml`, in slide-number order.
fn slide_xml(pptx: &[u8]) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(pptx)).expect("pptx is a valid zip");
    // Collect (name, xml) for slide parts, then sort by name so slide2 never precedes slide1.
    let mut slides: Vec<(String, String)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("zip entry");
        let name = file.name().to_string();
        // Match the slide parts only — not their rels, layouts, or the master.
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            let mut xml = String::new();
            file.read_to_string(&mut xml).expect("slide xml is UTF-8");
            slides.push((name, xml));
        }
    }
    slides.sort_by(|a, b| a.0.cmp(&b.0));
    slides.into_iter().map(|(_, xml)| xml).collect()
}

#[test]
fn each_level_one_heading_starts_a_slide() {
    // Two `#` headings → exactly two slides, each titled after its heading.
    let bytes = markdown_to_pptx_bytes("# Intro\n\nbody\n\n# Details\n\nmore").expect("converts");
    let slides = slide_xml(&bytes);
    assert_eq!(slides.len(), 2, "expected one slide per level-1 heading");
    assert!(
        slides[0].contains("Intro"),
        "slide 1 title missing: {}",
        slides[0]
    );
    assert!(
        slides[1].contains("Details"),
        "slide 2 title missing: {}",
        slides[1]
    );
}

#[test]
fn paragraphs_and_lists_become_bullets() {
    // A paragraph and list items on the same slide all surface as bullet text.
    let md = "# Slide\n\nA paragraph.\n\n- alpha\n- beta";
    let bytes = markdown_to_pptx_bytes(md).expect("converts");
    let xml = slide_xml(&bytes).remove(0);
    for needle in ["A paragraph.", "alpha", "beta"] {
        assert!(xml.contains(needle), "bullet '{needle}' missing: {xml}");
    }
}

#[test]
fn inline_formatting_is_flattened_to_text() {
    // Bold/italic/code carry no rich runs through the quick API; their text must still appear.
    let bytes = markdown_to_pptx_bytes("# T\n\nsome **bold** and `code`").expect("converts");
    let xml = slide_xml(&bytes).remove(0);
    assert!(
        xml.contains("bold") && xml.contains("code"),
        "flattened inline text missing: {xml}"
    );
}

#[test]
fn content_before_the_first_heading_gets_its_own_slide() {
    // Body that precedes any `#` still lands on a slide (titled after the deck default).
    let bytes = markdown_to_pptx_bytes("Just a line, no heading.").expect("converts");
    let slides = slide_xml(&bytes);
    assert_eq!(slides.len(), 1, "leading body should produce one slide");
    assert!(
        slides[0].contains("Just a line, no heading."),
        "leading body text missing: {}",
        slides[0]
    );
}

#[test]
fn produces_a_valid_pptx_zip() {
    let bytes = markdown_to_pptx_bytes("# Hi").expect("converts");
    // Every zip (and thus every .pptx) starts with the local-file-header magic `PK\x03\x04`.
    assert_eq!(&bytes[..4], b"PK\x03\x04");
}

#[test]
fn pptx_conversion_is_idempotent() {
    // Same idempotence contract as the DOCX path: identical Markdown → identical bytes.
    let md = "# One\n\n- a\n\n# Two\n\n1. b";
    let first = markdown_to_pptx_bytes(md).expect("converts");
    let second = markdown_to_pptx_bytes(md).expect("converts");
    assert_eq!(
        first, second,
        "identical input produced differing .pptx bytes"
    );
}

#[test]
fn pptx_conversion_is_idempotent_under_concurrency() {
    // ppt-rs pulls in `uuid`; guard against any hidden process-global randomness by converting
    // from many threads at once and requiring a single distinct byte string (the same regression
    // shape that caught docx-rs's global paragraph-id counter on the DOCX side).
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    let md = "# One\n\n- a\n- b\n\n# Two\n\n1. c";
    let reference = Arc::new(markdown_to_pptx_bytes(md).expect("converts"));

    let mut handles = Vec::new();
    for _ in 0..16 {
        let reference = Arc::clone(&reference);
        handles.push(thread::spawn(move || {
            for _ in 0..8 {
                let bytes = markdown_to_pptx_bytes(md).expect("converts");
                assert_eq!(bytes, *reference, "concurrent .pptx conversion diverged");
            }
        }));
    }
    for handle in handles {
        handle
            .join()
            .expect("worker thread panicked on a byte mismatch");
    }

    let distinct: HashSet<Vec<u8>> = (0..32)
        .map(|_| markdown_to_pptx_bytes(md).expect("converts"))
        .collect();
    assert_eq!(
        distinct.len(),
        1,
        "pptx conversion is not byte-deterministic"
    );
}
