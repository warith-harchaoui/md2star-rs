//! Citations and the bibliography in DOCX, compared with Pandoc's citeproc output for the
//! same fixtures.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use md2star_rs::{convert, Format, Options, Rendered};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn options() -> Options {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o
}

fn document(out: &Rendered) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// The visible text of each paragraph.
fn paragraphs(xml: &str) -> Vec<String> {
    xml.split("<w:p>")
        .skip(1)
        .map(|p| {
            let mut text = String::new();
            let mut rest = p;
            while let Some(at) = rest.find("<w:t xml:space=\"preserve\">") {
                rest = &rest[at + "<w:t xml:space=\"preserve\">".len()..];
                let end = rest.find("</w:t>").unwrap();
                text.push_str(&rest[..end]);
                rest = &rest[end..];
            }
            text.replace("&amp;", "&")
        })
        .filter(|t| !t.is_empty())
        .collect()
}

#[test]
fn the_bibliography_fixture_matches_pandoc() {
    let md = std::fs::read_to_string(fixtures().join("bibliography.md")).unwrap();
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let xml = document(&out);
    let text = paragraphs(&xml);
    // Pandoc 3.6 --citeproc on the same file (chicago-author-date, its default).
    assert!(
        text.iter()
            .any(|p| p.contains("causal-inference framework of Pearl (2009) throughout")),
        "{text:#?}"
    );
    assert!(
        text.iter()
            .any(|p| p.contains("foundational treatment in (Pearl 2000).")),
        "{text:#?}"
    );
    let n = text.len();
    assert_eq!(text[n - 3], "Bibliography", "md2star's default heading");
    assert_eq!(
        &text[n - 2..],
        &[
            "Pearl, Judea. 2000. Causality: Models, Reasoning, and Inference. 1st ed. Cambridge University Press.",
            "———. 2009. Causality: Models, Reasoning, and Inference. 2nd ed. Cambridge University Press.",
        ]
    );
}

#[test]
fn entries_are_bookmarked_in_the_bibliography_style() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert("See [@pearl2009].\n", Format::Docx, &o).unwrap();
    let xml = document(&out);
    assert!(xml.contains("<w:pStyle w:val=\"Heading1\"/></w:pPr><w:bookmarkStart w:id=\"1\" w:name=\"bibliography\"/>"));
    assert!(xml.contains("<w:bookmarkStart w:id=\"2\" w:name=\"refs\"/>"));
    assert!(xml.contains("<w:bookmarkStart w:id=\"3\" w:name=\"ref-pearl2009\"/><w:p><w:pPr><w:pStyle w:val=\"Bibliography\"/>"));
    // The title in italics.
    assert!(xml.contains("<w:i/><w:iCs/></w:rPr><w:t xml:space=\"preserve\">Causality: Models, Reasoning, and Inference</w:t>"));
}

#[test]
fn the_numeric_style_is_chosen_by_name() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    o.citation_style = Some("numeric".into());
    let out = convert(
        "A [@pearl2009, p. 3], B [@pearl2000; @pearl2009].\n",
        Format::Docx,
        &o,
    )
    .unwrap();
    let text = paragraphs(&document(&out));
    assert_eq!(text[0], "A [1, p. 3], B [1], [2].");
    assert!(text.iter().any(|p| p == "[1] J. Pearl, Causality: Models, Reasoning, and Inference, 2nd ed. Cambridge University Press, 2009."));
    // `csl: ieee.csl` in the front matter picks the same style.
    let out = convert(
        "---\ncsl: styles/ieee.csl\nbibliography: refs.bib\n---\n[@pearl2000]\n",
        Format::Docx,
        &options(),
    )
    .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(paragraphs(&document(&out))[0], "[1]");
}

#[test]
fn an_unknown_style_falls_back_with_a_warning() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    o.citation_style = Some("chicago-fullnote-bibliography.csl".into());
    let out = convert("[@pearl2000]\n", Format::Docx, &o).unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "citation-style-unknown"));
    assert_eq!(paragraphs(&document(&out))[0], "(Pearl 2000)");
}

#[test]
fn an_unknown_key_is_bold_as_written_with_a_warning() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert("See [@nobody; @pearl2000].\n", Format::Docx, &o).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].code, "citation-not-found");
    let xml = document(&out);
    assert!(xml.contains(
        "<w:rPr><w:b/><w:bCs/></w:rPr><w:t xml:space=\"preserve\">[@nobody; @pearl2000]</w:t>"
    ));
}

#[test]
fn the_heading_follows_options_then_front_matter_then_language() {
    let bib = fixtures().join("refs.bib");
    let run = |md: &str, name: Option<&str>| {
        let mut o = options();
        o.bibliography = vec![bib.clone()];
        o.bibliography_name = name.map(String::from);
        let out = convert(md, Format::Docx, &o).unwrap();
        paragraphs(&document(&out))
    };
    assert!(run("[@pearl2000]\n", Some("Sources")).contains(&"Sources".to_string()));
    assert!(run(
        "---\nreference-section-title: Works cited\n---\n[@pearl2000]\n",
        None
    )
    .contains(&"Works cited".to_string()));
    assert!(
        run("---\nlang: fr-FR\n---\n[@pearl2000]\n", None).contains(&"Bibliographie".to_string())
    );
}

#[test]
fn a_missing_bibliography_is_a_warning_and_citations_stay_as_written() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("nowhere.bib")];
    let out = convert("See [@pearl2000].\n", Format::Docx, &o).unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "bibliography-unreadable"));
    assert_eq!(paragraphs(&document(&out))[0], "See [@pearl2000].");
}

#[test]
fn without_a_bibliography_citations_are_left_as_written() {
    let out = convert(
        "See [@pearl2000, p. 3] and @pearl2009.\n",
        Format::Docx,
        &Options::default(),
    )
    .unwrap();
    assert!(out.warnings.is_empty());
    assert_eq!(
        paragraphs(&document(&out))[0],
        "See [@pearl2000, p. 3] and @pearl2009."
    );
}

#[test]
fn the_large_fixture_bibliography_renders_every_entry() {
    let src = std::fs::read_to_string(fixtures().join("references.bib")).unwrap();
    let keys: Vec<String> = src
        .lines()
        .filter_map(|l| l.strip_prefix('@'))
        .filter_map(|l| l.split_once('{'))
        .map(|(_, k)| k.trim_end_matches(',').to_string())
        .collect();
    let md: String = keys.iter().map(|k| format!("[@{k}]\n\n")).collect();
    let mut o = options();
    o.bibliography = vec![fixtures().join("references.bib")];
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let xml = document(&out);
    assert_eq!(
        xml.matches("<w:pStyle w:val=\"Bibliography\"/>").count(),
        keys.len()
    );
    let text = paragraphs(&xml);
    // Lines Pandoc 3.6 writes identically (chicago-author-date).
    for line in [
        "(Golub and Loan 2013)",
        "(Bouveyron et al. 2021)",
        "(Goodfellow, Bengio, and Courville 2016)",
        "Golub, Gene H., and Charles F. Van Loan. 2013. Matrix Computations, 4th Edition. https://www.press.jhu.edu/books/title/10678/matrix-computations.",
        "Prado, Marcos Lopez de. 2018. Advances in Financial Machine Learning. https://www.amazon.com/Advances-Financial-Machine-Learning-Marcos/dp/1119482089.",
        "Cook, Alexis, and Kaggle. 2017. “Kaggle Learn.” https://www.kaggle.com/learn.",
        "Goldberg, Yoav. 2017. Neural Network Methods in Natural Language Processing. https://u.cs.biu.ac.il/~yogo/nnlp.pdf.",
    ] {
        assert!(text.iter().any(|t| t == line), "missing: {line}");
    }
}
