//! `md2star-rs twin` end to end: the Markdown lands next to its `<stem>_media/` folder
//! and a PDF is refused with the reason.

#![cfg(feature = "cli")]

mod docx_common;

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_md2star-rs"))
}

#[test]
fn twin_writes_markdown_and_media_beside_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python-reference/with_bib.pptx");
    let input = dir.path().join("talk.pptx");
    std::fs::copy(src, &input).unwrap();
    let out = bin().args(["twin", "-q"]).arg(&input).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = std::fs::read_to_string(dir.path().join("talk.md")).unwrap();
    assert!(md.starts_with("---\ntitle: Bibliography Test\n"), "{md}");
}

#[test]
fn twin_media_folder_is_named_after_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let png = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images/pixel-4x3.png"),
    )
    .unwrap();
    // A Word document with one picture, written by hand (not by our own writer).
    let docx = docx_common::document_with_picture(&png, 4 * 9525, 3 * 9525);
    std::fs::write(dir.path().join("report.docx"), docx).unwrap();
    let out_dir = dir.path().join("twin");
    let ok = bin()
        .args(["twin", "-q"])
        .arg(dir.path().join("report.docx"))
        .arg("--out")
        .arg(&out_dir)
        .status()
        .unwrap();
    assert!(ok.success());
    let twin = std::fs::read_to_string(out_dir.join("report.md")).unwrap();
    assert!(twin.starts_with("![](report_media/"), "{twin}");
    let rel = twin.trim().trim_start_matches("![](").trim_end_matches(')');
    assert_eq!(std::fs::read(out_dir.join(rel)).unwrap(), png);
}

#[test]
fn twin_refuses_pdf_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("paper.pdf");
    std::fs::write(&pdf, b"%PDF-1.7\n").unwrap();
    let out = bin().arg("twin").arg(&pdf).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("OCR") && err.contains(".docx"), "{err}");
}
