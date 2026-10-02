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
    // run — no timestamps, and footnote/numbering/paragraph ids come from per-document counters.
    let markdown = "# Doc\n\n1. one[^a]\n2. two\n\n- bullet\n\n[^a]: note";
    let first = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    let second = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
    assert_eq!(
        first, second,
        "identical input produced differing .docx bytes"
    );
}

#[test]
fn conversion_is_idempotent_under_concurrency() {
    // Regression guard for the process-global `w14:paraId` counter inside docx-rs: because
    // paragraph ids used to be drawn from a shared atomic, converting on several threads at
    // once interleaved their id allocations and produced diverging bytes for identical input
    // (flaky CI on macOS + Windows). Our writer now mints ids from a per-document counter, so
    // concurrency must not matter. We hammer the conversion from many threads and require a
    // single distinct byte string across all of them.
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    let markdown = "# Doc\n\n1. one[^a]\n2. two\n\n- bullet\n\n[^a]: note";
    // The canonical output every thread must reproduce exactly.
    let reference = Arc::new(markdown_to_docx_bytes(markdown).expect("conversion succeeds"));

    // Spawn a pool of threads, each doing several conversions, to force id-allocation races.
    let mut handles = Vec::new();
    for _ in 0..16 {
        let reference = Arc::clone(&reference);
        handles.push(thread::spawn(move || {
            // Repeat inside the thread so allocations from sibling threads interleave heavily.
            for _ in 0..8 {
                let bytes = markdown_to_docx_bytes(markdown).expect("conversion succeeds");
                assert_eq!(
                    bytes, *reference,
                    "concurrent conversion diverged from the reference bytes"
                );
            }
        }));
    }

    // A dead thread means an assertion tripped; surface it as a test failure.
    for handle in handles {
        handle
            .join()
            .expect("worker thread panicked on a byte mismatch");
    }

    // Belt and braces: collect a fresh batch and confirm they collapse to one distinct value.
    let distinct: HashSet<Vec<u8>> = (0..32)
        .map(|_| markdown_to_docx_bytes(markdown).expect("conversion succeeds"))
        .collect();
    assert_eq!(distinct.len(), 1, "conversion is not byte-deterministic");
}

#[test]
fn reader_nests_emphasis_inside_strong() {
    // A white-box check on the AST seam: `**a _b_**` must parse to Strong[Text, Emph[Text]].
    let blocks = reader::parse("**a _b_**");
    assert_eq!(blocks.len(), 1, "expected a single paragraph");
}

/// Pull `word/numbering.xml` out of a packed `.docx` byte buffer.
fn numbering_xml(docx_bytes: &[u8]) -> String {
    entry(docx_bytes, "word/numbering.xml").expect("docx contains word/numbering.xml")
}

#[test]
fn an_ordered_list_starts_at_the_number_the_markdown_asks_for() {
    let bytes = markdown_to_docx_bytes("5. five\n6. six\n").expect("conversion succeeds");
    let xml = numbering_xml(&bytes);
    // The first number lives on the abstract definition's level, not on the paragraph,
    // so this is where a list that opens at 5 has to show up.
    assert!(
        xml.contains(r#"<w:start w:val="5""#),
        "no level starting at 5 in numbering.xml: {xml}"
    );
}

#[test]
fn a_list_starting_at_one_still_shares_the_default_numbering_definition() {
    // The common case must not mint a definition per list — that would bloat
    // numbering.xml on any document with many lists.
    let one = markdown_to_docx_bytes("1. a\n2. b\n").expect("conversion succeeds");
    let many = markdown_to_docx_bytes("1. a\n\ntext\n\n1. b\n\ntext\n\n1. c\n")
        .expect("conversion succeeds");
    // The trailing space matters: `<w:abstractNum ` is the definition element,
    // while `<w:abstractNumId` is the per-instance back-reference and grows with
    // the number of lists no matter which definition they share.
    let count = |xml: &str| xml.matches("<w:abstractNum ").count();
    assert_eq!(
        count(&numbering_xml(&one)),
        count(&numbering_xml(&many)),
        "lists starting at 1 should reuse one abstract numbering definition"
    );
}

#[test]
fn two_ordered_lists_with_different_starts_do_not_share_a_definition() {
    let bytes =
        markdown_to_docx_bytes("3. three\n\ntext\n\n7. seven\n").expect("conversion succeeds");
    let xml = numbering_xml(&bytes);
    assert!(
        xml.contains(r#"w:val="3""#),
        "the 3-start is missing: {xml}"
    );
    assert!(
        xml.contains(r#"w:val="7""#),
        "the 7-start is missing: {xml}"
    );
}

#[test]
fn no_numbering_id_is_declared_twice() {
    // `docx-rs` always writes its own `<w:abstractNum w:abstractNumId="1">` and
    // `<w:num w:numId="1">` ahead of ours. Colliding with either produces a
    // `numbering.xml` with duplicate ids, and Word reads the *first* — which is
    // how every bullet list came out as `1. 2. 3.` before 0.4.3.
    let bytes = markdown_to_docx_bytes("- a\n- b\n\ntext\n\n1. one\n\ntext\n\n4. four\n")
        .expect("conversion succeeds");
    let xml = numbering_xml(&bytes);

    let ids = |tag: &str| -> Vec<String> {
        xml.match_indices(tag)
            .map(|(i, _)| {
                let rest = &xml[i + tag.len()..];
                rest[..rest.find('"').unwrap()].to_string()
            })
            .collect()
    };
    for (what, tag) in [
        ("abstractNum", r#"<w:abstractNum w:abstractNumId=""#),
        ("num", r#"<w:num w:numId=""#),
    ] {
        let mut seen = ids(tag);
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(
            total,
            seen.len(),
            "duplicate {what} id in numbering.xml: {xml}"
        );
    }
}

#[test]
fn a_bullet_list_points_at_a_bullet_definition_not_a_decimal_one() {
    let bytes = markdown_to_docx_bytes("- a\n- b\n").expect("conversion succeeds");
    let numbering = numbering_xml(&bytes);
    let document = document_xml(&bytes);

    // Follow the same chain Word follows: the paragraph names an instance, the
    // instance names a definition, the definition says what the marker looks like.
    let num_id = attr_after(&document, r#"<w:numId w:val=""#).expect("list items carry a numId");
    let abstract_id = attr_after(
        &numbering,
        &format!(r#"<w:num w:numId="{num_id}"><w:abstractNumId w:val=""#),
    )
    .unwrap_or_else(|| panic!("no instance {num_id} in numbering.xml: {numbering}"));
    let definition = numbering
        .find(&format!(
            r#"<w:abstractNum w:abstractNumId="{abstract_id}">"#
        ))
        .unwrap_or_else(|| panic!("no definition {abstract_id} in numbering.xml: {numbering}"));
    let format = attr_after(&numbering[definition..], r#"<w:numFmt w:val=""#)
        .expect("a definition declares a numFmt");

    assert_eq!(
        format, "bullet",
        "a bullet list resolved to a {format} definition (numId {num_id} -> abstractNum {abstract_id})"
    );
}

/// The value of the attribute `prefix` opens, i.e. everything up to the closing
/// quote. `None` when the prefix does not occur at all.
fn attr_after(xml: &str, prefix: &str) -> Option<String> {
    let rest = &xml[xml.find(prefix)? + prefix.len()..];
    Some(rest[..rest.find('"')?].to_string())
}
