//! `--reference-doc` with the full DOCX writer: the reference's parts, section, styles
//! and numbering are kept, the styles it lacks are added, nothing of its body survives.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use md2star_rs::{convert, Error, Format, Options, Rendered};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn md2star_template() -> Vec<u8> {
    std::fs::read(fixtures().join("templates/template.docx")).unwrap()
}

fn with_reference(md: &str, reference: Vec<u8>) -> Rendered {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o.reference_doc = Some(reference);
    convert(md, Format::Docx, &o).expect("converts")
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|i| {
            let mut f = archive.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().to_string(), data)
        })
        .collect()
}

fn text(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(
        parts
            .get(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .clone(),
    )
    .unwrap()
}

/// Build a `.docx` from named parts.
fn package(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

/// A minimal reference document with the given styles and body.
fn minimal_reference(styles: &str, body: &str) -> Vec<u8> {
    package(&[
        ("[Content_Types].xml", "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml\"/></Types>"),
        ("_rels/.rels", "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"),
        ("word/document.xml", &format!("<?xml version=\"1.0\"?><w:document {W}><w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1800\" w:bottom=\"1440\" w:left=\"1800\"/></w:sectPr></w:body></w:document>")),
        ("word/styles.xml", &format!("<?xml version=\"1.0\"?><w:styles {W}>{styles}</w:styles>")),
        ("word/numbering.xml", &format!("<?xml version=\"1.0\"?><w:numbering {W}><w:abstractNum w:abstractNumId=\"7\"/><w:num w:numId=\"9\"><w:abstractNumId w:val=\"7\"/></w:num></w:numbering>")),
    ])
}

#[test]
fn md2stars_template_styles_the_whole_document() {
    let md = std::fs::read_to_string(fixtures().join("syntax.md")).unwrap();
    let out = with_reference(&md, md2star_template());
    let p = parts(&out.bytes);
    let template = parts(&md2star_template());
    // Its parts come along untouched: theme, footers, fonts, settings, custom XML.
    for name in [
        "word/theme/theme1.xml",
        "word/footer1.xml",
        "word/footer2.xml",
        "word/fontTable.xml",
        "word/settings.xml",
        "customXml/item1.xml",
    ] {
        assert_eq!(p[name], template[name], "{name}");
    }
    // The final section keeps its page size and footers, whose relationships survive.
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains("<w:pgSz w:w=\"11900\" w:h=\"16820\"/>"));
    let rels = text(&p, "word/_rels/document.xml.rels");
    for footer_id in ["rId9", "rId10"] {
        assert!(xml.contains(&format!("r:id=\"{footer_id}\"")));
        assert!(rels.contains(&format!("Id=\"{footer_id}\"")), "{footer_id}");
    }
    // Nothing of the template's body.
    let template_rels = text(&template, "word/_rels/document.xml.rels");
    assert!(template_rels.contains("md2star.dev"));
    assert!(
        !rels.contains("md2star.dev"),
        "the body's hyperlink went with it"
    );
    // Styles: the template's own are used, not duplicated; md2star's table styles
    // (absent from the template) are added.
    let styles = text(&p, "word/styles.xml");
    for id in [
        "Title",
        "Heading1",
        "FirstParagraph",
        "Compact",
        "ImageCaption",
        "Bibliography",
        "SourceCode",
        "VerbatimChar",
        "Hyperlink",
    ] {
        assert_eq!(
            styles.matches(&format!("w:styleId=\"{id}\"")).count(),
            1,
            "{id}"
        );
    }
    assert!(styles.contains("w:styleId=\"MyTable\""));
    assert!(text(&template, "word/styles.xml").len() < styles.len());
    // The language is set on the template's defaults.
    assert!(styles.contains("<w:lang w:val=\"en-US\"/>"));
}

#[test]
fn numbering_ids_stay_clear_of_the_references() {
    let out = with_reference("1. one\n2. two\n\n- a\n", minimal_reference("", ""));
    let p = parts(&out.bytes);
    let numbering = text(&p, "word/numbering.xml");
    assert!(
        numbering.contains("<w:abstractNum w:abstractNumId=\"7\"/>"),
        "the reference's kept"
    );
    assert_eq!(numbering.matches("w:numId=\"9\"").count(), 1);
    assert!(numbering.contains("<w:abstractNum w:abstractNumId=\"8\">"));
    assert!(numbering.contains("<w:num w:numId=\"10\">"));
    let xml = text(&p, "word/document.xml");
    assert!(!xml.contains("<w:numId w:val=\"9\"/>"));
}

#[test]
fn missing_styles_are_added_with_their_bases() {
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#;
    let out = with_reference(
        "# T\n\n## Section\n\nText.\n",
        minimal_reference(styles, ""),
    );
    let s = text(&parts(&out.bytes), "word/styles.xml");
    for id in ["Title", "Heading2", "FirstParagraph", "BodyText"] {
        assert_eq!(s.matches(&format!("w:styleId=\"{id}\"")).count(), 1, "{id}");
    }
    assert_eq!(s.matches("w:styleId=\"Normal\"").count(), 1);
}

#[test]
fn a_localised_template_is_used_through_its_style_names() {
    // Word in French: id `Titre1`, name `heading 1`.
    let styles = r#"<w:style w:type="paragraph" w:styleId="Titre1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Titre"><w:name w:val="Title"/></w:style>"#;
    let out = with_reference(
        "# Titre du document\n\n# Partie\n",
        minimal_reference(styles, ""),
    );
    let p = parts(&out.bytes);
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains("<w:pStyle w:val=\"Titre\"/>"));
    assert!(xml.contains("<w:pStyle w:val=\"Titre1\"/>"));
    assert!(!text(&p, "word/styles.xml").contains("w:styleId=\"Heading1\""));
}

#[test]
fn a_template_document_becomes_a_document_and_its_body_media_is_dropped() {
    let body = "<w:p><w:r><w:t>TEMPLATE BODY TEXT</w:t></w:r></w:p>";
    let mut reference = parts(&minimal_reference("", body));
    reference.insert(
        "word/media/logo.png".into(),
        b"not used by any kept part".to_vec(),
    );
    let entries: Vec<(String, String)> = reference
        .into_iter()
        .map(|(k, v)| (k, String::from_utf8(v).unwrap()))
        .collect();
    let refs: Vec<(&str, &str)> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let out = with_reference("Hello.\n", package(&refs));
    let p = parts(&out.bytes);
    assert!(!text(&p, "word/document.xml").contains("TEMPLATE BODY TEXT"));
    assert!(!p.contains_key("word/media/logo.png"));
    // A .dotx-style content type is turned into a document's.
    let ct = text(&p, "[Content_Types].xml");
    assert!(ct.contains("PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\""));
    // US Letter with 1.25 in margins: the text column used for images is 6 in.
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
}

#[test]
fn images_are_sized_to_the_reference_page() {
    let out = with_reference(
        "![wide](images/pixel-4x3.png){width=40in}\n",
        minimal_reference("", ""),
    );
    let xml = text(&parts(&out.bytes), "word/document.xml");
    let width = (12240 - 2 * 1800) * 635;
    assert!(xml.contains(&format!("<wp:extent cx=\"{width}\"")), "{xml}");
}

#[test]
fn a_reference_conversion_is_idempotent() {
    let md = std::fs::read_to_string(fixtures().join("syntax.md")).unwrap();
    let a = with_reference(&md, md2star_template()).bytes;
    let b = with_reference(&md, md2star_template()).bytes;
    assert_eq!(a, b);
}

#[test]
fn an_unreadable_reference_is_a_template_error() {
    let mut o = Options::default();
    o.reference_doc = Some(b"not a zip".to_vec());
    assert!(matches!(
        convert("x", Format::Docx, &o),
        Err(Error::Template(_))
    ));
    o.reference_doc = Some(package(&[("hello.txt", "no document part")]));
    assert!(matches!(
        convert("x", Format::Docx, &o),
        Err(Error::Template(_))
    ));
}
