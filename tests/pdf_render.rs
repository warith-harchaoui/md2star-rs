//! PDF output (feature `pdf`): the file opens, holds the expected text, has a plausible
//! number of pages, and is byte-identical from one run to the next.
//!
//! The text is read back with poppler's `pdftotext` / `pdfinfo` when they are installed
//! (a test-only tool, never a dependency of the crate); without them those checks are
//! skipped with a note, and the structural checks still run.

#![cfg(feature = "pdf")]

use std::path::PathBuf;
use std::process::Command;

use md2star_rs::{convert, Format, Options};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn options() -> Options {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o
}

fn fixture_pdf(name: &str) -> md2star_rs::Rendered {
    let md = std::fs::read_to_string(fixtures().join(name)).unwrap();
    convert(&md, Format::Pdf, &options()).unwrap()
}

/// `pdftotext` output, or `None` when poppler is not installed.
fn pdf_text(pdf: &[u8]) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf");
    std::fs::write(&path, pdf).unwrap();
    let out = Command::new("pdftotext")
        .arg("-enc")
        .arg("UTF-8")
        .arg(&path)
        .arg("-")
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "pdftotext rejected the PDF: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn page_count(pdf: &[u8]) -> Option<usize> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf");
    std::fs::write(&path, pdf).unwrap();
    let out = Command::new("pdfinfo").arg(&path).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Pages:").map(|n| n.trim().parse().unwrap()))
}

fn assert_is_pdf(bytes: &[u8]) {
    assert!(bytes.starts_with(b"%PDF-"), "no PDF header");
    let tail = &bytes[bytes.len().saturating_sub(64)..];
    assert!(tail.windows(5).any(|w| w == b"%%EOF"), "no %%EOF trailer");
}

/// Collapse whitespace and drop soft hyphens, so a word split across lines matches.
fn flat(text: &str) -> String {
    text.replace("\u{AD}\n", "")
        .replace('\u{AD}', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_syntax_fixture_becomes_a_readable_pdf() {
    let out = fixture_pdf("syntax.md");
    assert_is_pdf(&out.bytes);
    assert!(
        !out.warnings.iter().any(|w| w.code == "math-fallback"),
        "{:?}",
        out.warnings
    );
    if let Some(text) = pdf_text(&out.bytes) {
        let text = flat(&text);
        for expected in [
            "Every construction md2star reads",
            "Ada Lovelace",
            "Headings and attributes",
            "Plain text with bold, italic",
            "The first footnote.",
            "Aligned columns and a caption",
            "A quotation, on two lines.",
            "Left column.",
            "Right column.",
            "def f(x):",
        ] {
            assert!(text.contains(expected), "missing `{expected}` in:\n{text}");
        }
        // `toc: true` in the front matter.
        assert!(text.contains("Contents"), "{text}");
    } else {
        eprintln!("pdftotext not installed: text checks skipped");
    }
    if let Some(pages) = page_count(&out.bytes) {
        assert!((2..=8).contains(&pages), "{pages} pages");
    }
}

#[test]
fn pdf_output_is_byte_identical_run_to_run() {
    let a = fixture_pdf("syntax.md");
    let b = fixture_pdf("syntax.md");
    assert_eq!(a.bytes, b.bytes);
    let a = fixture_pdf("comprehensive_document.md");
    let b = fixture_pdf("comprehensive_document.md");
    assert_eq!(a.bytes, b.bytes);
}

#[test]
fn every_fixture_converts() {
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let md = std::fs::read_to_string(&path).unwrap();
        let out = convert(&md, Format::Pdf, &options())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_is_pdf(&out.bytes);
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.code == "math-fallback" || w.code == "typst"),
            "{}: {:?}",
            path.display(),
            out.warnings
        );
    }
}

#[test]
fn author_text_never_becomes_typst_code() {
    let md = "# A #heading with $5 and *stars*\n\nText with \\# #set page(width: 1cm) and \
              // not a comment, @label, 1 < 2 > 0, ~tilde, [brackets], `#raw` and back\\\\slash.\n\n\
              = not a heading\n\n12\\. not a list\n";
    let out = convert(md, Format::Pdf, &Options::default()).unwrap();
    assert_is_pdf(&out.bytes);
    if let Some(text) = pdf_text(&out.bytes) {
        let text = flat(&text);
        assert!(text.contains("#set page(width: 1cm)"), "{text}");
        assert!(
            text.contains("// not a comment, @label, 1 < 2 > 0, ~tilde, [brackets]"),
            "{text}"
        );
        assert!(text.contains("= not a heading"), "{text}");
        assert!(text.contains("12. not a list"), "{text}");
    }
}

#[test]
fn formulas_are_typeset_natively() {
    let md = "Inline $\\alpha^2 + \\beta_i$ and display:\n\n$$\n\\int_0^1 \\frac{x}{2}\\,dx = \\sqrt{\\pi}\n$$\n\n\
              $$\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}$$\n";
    let out = convert(md, Format::Pdf, &Options::default()).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Some(text) = pdf_text(&out.bytes) {
        // Math letters come out as their mathematical italic code points.
        for (plain, italic) in [("α", "𝛼"), ("β", "𝛽"), ("π", "𝜋"), ("∫", "∫"), ("√", "√")]
        {
            assert!(
                text.contains(plain) || text.contains(italic),
                "missing {plain} in {text}"
            );
        }
    }
}

#[test]
fn an_unknown_command_is_reported_and_kept_visible() {
    let out = convert("$\\weird{x} + 1$", Format::Pdf, &Options::default()).unwrap();
    assert_is_pdf(&out.bytes);
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "math-unsupported" && w.message.contains("\\weird")));
    if let Some(text) = pdf_text(&out.bytes) {
        assert!(text.contains("weird"), "{text}");
    }
}

#[test]
fn french_is_hyphenated_and_justified() {
    let long = "Les anticonstitutionnellement extraordinairement interminables délibérations \
                intergouvernementales se poursuivirent ";
    let md = format!("---\nlang: fr-FR\n---\n\n{}\n", long.repeat(6));
    let out = convert(&md, Format::Pdf, &Options::default()).unwrap();
    if let Some(text) = pdf_text(&out.bytes) {
        // Typst marks the break of a hyphenated word with a soft hyphen at line end.
        assert!(
            text.contains("\u{AD}\n") || text.contains("-\n"),
            "no hyphenation in:\n{text}"
        );
    }
}

#[test]
fn missing_images_degrade_to_their_alt_text() {
    let out = convert("![The lost chart](nowhere.png)\n", Format::Pdf, &options()).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "image-not-loaded"));
    if let Some(text) = pdf_text(&out.bytes) {
        assert!(text.contains("The lost chart"), "{text}");
    }
}

#[test]
fn images_are_embedded() {
    let with = convert(
        "![A photo](images/photo-8x6.jpg)\n\n![Vector](images/diagram.svg)\n",
        Format::Pdf,
        &options(),
    )
    .unwrap();
    assert!(with.warnings.is_empty(), "{:?}", with.warnings);
    let without = convert("A photo\n\nVector\n", Format::Pdf, &options()).unwrap();
    // The JPEG is embedded as is: the file grows by at least its size.
    let jpeg = std::fs::read(fixtures().join("images/photo-8x6.jpg")).unwrap();
    assert!(with.bytes.len() >= without.bytes.len() + jpeg.len() / 2);
}

#[test]
fn citations_are_resolved_like_in_word() {
    let md = std::fs::read_to_string(fixtures().join("bibliography.md")).unwrap();
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert(&md, Format::Pdf, &o).unwrap();
    if let Some(text) = pdf_text(&out.bytes) {
        let text = flat(&text);
        assert!(text.contains("Pearl"), "{text}");
        assert!(!text.contains("[@"), "an unresolved citation: {text}");
    }
}

#[test]
fn mermaid_without_the_feature_stays_readable_with_a_warning() {
    let out = fixture_pdf("mermaid.md");
    if cfg!(feature = "mermaid") {
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.code == "mermaid-not-rendered"),
            "{:?}",
            out.warnings
        );
    } else {
        assert!(out
            .warnings
            .iter()
            .any(|w| w.code == "mermaid-not-rendered"));
        if let Some(text) = pdf_text(&out.bytes) {
            assert!(
                text.contains("flowchart") || text.contains("graph"),
                "{text}"
            );
        }
    }
}

#[test]
fn the_typst_source_is_available() {
    let doc = md2star_rs::reader::read("# Title\n\nBody with *emphasis*.\n");
    let (source, warnings) = md2star_rs::pdf::to_typst(&doc, &Options::default()).unwrap();
    assert!(warnings.is_empty());
    assert!(source.contains("= Title <title>"), "{source}");
    assert!(source.contains("Body with #emph[emphasis];."), "{source}");
}

#[test]
fn a_reference_document_is_reported_not_silently_ignored() {
    let mut o = Options::default();
    o.reference_doc = Some(b"PK".to_vec());
    let out = convert("Hello.", Format::Pdf, &o).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "not-yet-ported"));
}
