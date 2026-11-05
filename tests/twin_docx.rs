//! `twin` for DOCX on hand-built documents: each test writes the exact OOXML a producer
//! (Word, Pandoc, md2star) emits for one construction and checks the Markdown that
//! comes back — so every recovered construction is pinned to its XML, not to whatever
//! one writer happens to produce today.

use std::io::{Cursor, Write};

use md2star_rs::twin::{self, SourceKind, TwinOptions};
use md2star_rs::Error;
use zip::write::SimpleFileOptions;

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0,
    1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xDE,
];

/// A .docx from its body XML plus optional parts (path, content).
fn docx(body: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let o = SimpleFileOptions::default();
        let mut put = |name: &str, data: &[u8]| {
            zip.start_file(name, o).unwrap();
            zip.write_all(data).unwrap();
        };
        put(
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        );
        put(
            "word/document.xml",
            format!(r#"<?xml version="1.0"?><w:document {W}><w:body>{body}</w:body></w:document>"#)
                .as_bytes(),
        );
        for (name, data) in parts {
            put(name, data);
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn twin_md(bytes: &[u8]) -> String {
    twin::to_markdown(bytes, SourceKind::Docx, &TwinOptions::default())
        .unwrap()
        .markdown
}

fn p(style: Option<&str>, runs: &str) -> String {
    match style {
        Some(s) => format!(r#"<w:p><w:pPr><w:pStyle w:val="{s}"/></w:pPr>{runs}</w:p>"#),
        None => format!("<w:p>{runs}</w:p>"),
    }
}

fn r(text: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

fn rp(props: &str, text: &str) -> String {
    format!(r#"<w:r><w:rPr>{props}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

const STYLES: &str = r#"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="fr-FR"/></w:rPr></w:rPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:styleId="Titre1"><w:name w:val="heading 1"/></w:style>
<w:style w:type="paragraph" w:styleId="MonTitre"><w:name w:val="Mon titre"/><w:basedOn w:val="Titre1"/></w:style>
<w:style w:type="paragraph" w:styleId="Plan"><w:name w:val="Plan"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style>
<w:style w:type="paragraph" w:styleId="Author"><w:name w:val="Author"/></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style>
<w:style w:type="paragraph" w:styleId="SourceCode"><w:name w:val="Source Code"/></w:style>
<w:style w:type="paragraph" w:styleId="ImageCaption"><w:name w:val="Image Caption"/></w:style>
<w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr></w:style>
<w:style w:type="character" w:styleId="VerbatimChar"><w:name w:val="Verbatim Char"/></w:style>
<w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/></w:style>
</w:styles>"#;

const NUMBERING: &str = r#"<?xml version="1.0"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num>
<w:num w:numId="6"><w:abstractNumId w:val="2"/></w:num>
<w:num w:numId="7"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

fn styled(body: &str) -> Vec<u8> {
    docx(
        body,
        &[
            ("word/styles.xml", STYLES.as_bytes()),
            ("word/numbering.xml", NUMBERING.as_bytes()),
        ],
    )
}

#[test]
fn headings_come_from_style_names_based_on_chains_and_outline_levels() {
    let body = [
        p(Some("Titre1"), &r("One")),
        p(Some("MonTitre"), &r("Inherited")),
        p(Some("Plan"), &r("Outline three")),
        r#"<w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Direct two</w:t></w:r></w:p>"#
            .to_string(),
        p(Some("Heading4"), &r("Undeclared id")),
    ]
    .concat();
    assert_eq!(
        twin_md(&styled(&body)),
        "# One\n\n# Inherited\n\n### Outline three\n\n## Direct two\n\n#### Undeclared id\n"
    );
}

#[test]
fn run_formatting_becomes_markdown() {
    let body = p(
        None,
        &[
            r("plain "),
            rp("<w:b/>", "bold"),
            r(" "),
            rp("<w:i/>", "italic"),
            r(" "),
            rp("<w:b/><w:i/>", "both"),
            r(" "),
            rp("<w:strike/>", "gone"),
            r(" H"),
            rp(r#"<w:vertAlign w:val="subscript"/>"#, "2"),
            r("O x"),
            rp(r#"<w:vertAlign w:val="superscript"/>"#, "2"),
            r(" "),
            rp(r#"<w:rStyle w:val="VerbatimChar"/>"#, "a_b"),
            r(" "),
            rp(
                r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/>"#,
                "mono",
            ),
            r(" "),
            rp(r#"<w:rStyle w:val="Strong"/>"#, "strong"),
            r(" "),
            rp(r#"<w:b w:val="0"/>"#, "not bold"),
        ]
        .concat(),
    );
    assert_eq!(
        twin_md(&styled(&body)),
        "plain **bold** *italic* ***both*** ~~gone~~ H~2~O x^2^ `a_b` `mono` **strong** not bold\n"
    );
}

#[test]
fn text_that_looks_like_markdown_is_escaped() {
    let body =
        p(None, &r("# not a heading, 2*3, [x], @key and $y$")) + &p(None, &r("1. not a list"));
    assert_eq!(
        twin_md(&styled(&body)),
        "\\# not a heading, 2\\*3, \\[x\\], \\@key and \\$y$\n\n1\\. not a list\n"
    );
}

#[test]
fn lists_follow_numbering_xml() {
    let item = |num: u32, lvl: u32, t: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr></w:pPr>{}</w:p>"#,
            r(t)
        )
    };
    let body = [
        item(5, 0, "apple"),
        item(5, 1, "first"),
        item(5, 1, "second"),
        item(5, 0, "pear"),
        p(None, &r("Between.")),
        item(6, 0, "three"),
        item(6, 0, "four"),
        p(Some("ListBullet"), &r("by style")),
        item(5, 0, "☒ done"),
    ]
    .concat();
    assert_eq!(
        twin_md(&styled(&body)),
        "- apple\n  1. first\n  2. second\n- pear\n\nBetween.\n\n3. three\n4. four\n\n- by style\n\n- [x] done\n"
    );
}

#[test]
fn tables_keep_cells_alignment_and_merges() {
    let cell = |jc: &str, extra: &str, t: &str| {
        format!(
            r#"<w:tc><w:tcPr>{extra}</w:tcPr><w:p><w:pPr><w:jc w:val="{jc}"/></w:pPr>{}</w:p></w:tc>"#,
            r(t)
        )
    };
    let body = format!(
        "<w:tbl><w:tr>{}{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>{}",
        cell("left", "", "Name"),
        cell("center", "", "a|b"),
        cell("right", "", "Score"),
        cell("left", r#"<w:gridSpan w:val="2"/>"#, "wide"),
        cell("right", "", "9"),
        p(Some("ImageCaption"), &r("Scores"))
    );
    assert_eq!(
        twin_md(&styled(&body)),
        "| Name | a\\|b | Score |\n|:---|:---:|---:|\n| wide |  | 9 |\n\nTable: Scores\n"
    );
}

#[test]
fn hyperlinks_relationships_anchors_and_fields() {
    let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://deraison.ai/a b" TargetMode="External"/></Relationships>"#;
    let body = p(
        None,
        &[
            format!(r#"<w:hyperlink r:id="rId9">{}</w:hyperlink>"#, r("site")),
            r(" and "),
            format!(r#"<w:hyperlink w:anchor="intro">{}</w:hyperlink>"#, r("intro")),
            r(" and "),
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> HYPERLINK "https://example.org" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"#.to_string(),
            rp("<w:b/>", "field"),
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#.to_string(),
        ]
        .concat(),
    );
    let bytes = docx(&body, &[("word/_rels/document.xml.rels", rels)]);
    assert_eq!(
        twin_md(&bytes),
        "[site](<https://deraison.ai/a b>) and [intro](#intro) and [**field**](https://example.org)\n"
    );
}

#[test]
fn footnotes_and_endnotes_are_numbered_by_first_reference() {
    let notes = br#"<?xml version="1.0"?><w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
<w:footnote w:id="7"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Seven, with </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>style</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p></w:footnote>
<w:footnote w:id="3"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Three.</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;
    let body = p(
        None,
        &[
            r("A"),
            r#"<w:r><w:footnoteReference w:id="7"/></w:r>"#.to_string(),
            r(" B"),
            r#"<w:r><w:footnoteReference w:id="3"/></w:r>"#.to_string(),
            r(" again"),
            r#"<w:r><w:footnoteReference w:id="7"/></w:r>"#.to_string(),
        ]
        .concat(),
    );
    let bytes = docx(&body, &[("word/footnotes.xml", notes)]);
    assert_eq!(
        twin_md(&bytes),
        "A[^1] B[^2] again[^1]\n\n[^1]: Seven, with *style*.\n\n    Second paragraph.\n\n[^2]: Three.\n"
    );
}

#[test]
fn images_are_extracted_with_alt_text_and_captions() {
    let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#;
    let drawing = r#"<w:r><w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="A pixel"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let body = p(None, drawing)
        + &p(
            None,
            &[r("Inline "), drawing.to_string(), r(" too.")].concat(),
        )
        + &p(None, drawing)
        + &p(Some("ImageCaption"), &r("The caption"));
    let bytes = docx(
        &body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/media/image1.png", PNG),
            ("word/styles.xml", STYLES.as_bytes()),
        ],
    );
    let twin = twin::to_markdown(
        &bytes,
        SourceKind::Docx,
        &TwinOptions {
            images: true,
            media_dir: "report_media".into(),
        },
    )
    .unwrap();
    assert_eq!(
        twin.markdown,
        "![A pixel](report_media/image1.png)\n\nInline ![A pixel](report_media/image1.png) too.\n\n![The caption](report_media/image1.png)\n"
    );
    // Extracted once, bytes intact.
    assert_eq!(
        twin.media,
        vec![("report_media/image1.png".to_string(), PNG.to_vec())]
    );

    let without = twin::to_markdown(
        &bytes,
        SourceKind::Docx,
        &TwinOptions {
            images: false,
            ..TwinOptions::default()
        },
    )
    .unwrap();
    assert!(without.media.is_empty());
    assert!(!without.markdown.contains("!["));
    assert!(without
        .warnings
        .iter()
        .any(|w| w.code == "twin-images-dropped"));
}

#[test]
fn equations_come_back_as_latex() {
    let frac = r#"<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>"#;
    let body = p(
        None,
        &[r("Inline "), format!("<m:oMath>{frac}</m:oMath>"), r(".")].concat(),
    ) + &p(
        None,
        &format!("<m:oMathPara><m:oMath>{frac}</m:oMath></m:oMathPara>"),
    ) + &p(
        None,
        "<m:oMath><m:strange><m:r><m:t>q</m:t></m:r></m:strange></m:oMath>",
    );
    let twin =
        twin::to_markdown(&docx(&body, &[]), SourceKind::Docx, &TwinOptions::default()).unwrap();
    assert_eq!(
        twin.markdown,
        "Inline $\\frac{a}{b}$.\n\n$$\n\\frac{a}{b}\n$$\n\n$q$\n"
    );
    assert!(twin
        .warnings
        .iter()
        .any(|w| w.code == "twin-math-unknown" && w.message.contains("strange")));
}

#[test]
fn quotes_code_and_rules() {
    let body = [
        p(Some("Quote"), &r("Quoted one.")),
        p(Some("Quote"), &r("Quoted two.")),
        p(Some("SourceCode"), &r("let x = 1;")),
        p(Some("SourceCode"), &r("")),
        p(Some("SourceCode"), &r("x += `1`;")),
        p(None, &r("————————")),
        p(
            None,
            &rp(
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/>"#,
                "mono line",
            ),
        ),
    ]
    .concat();
    assert_eq!(
        twin_md(&styled(&body)),
        "> Quoted one.\n>\n> Quoted two.\n\n```\nlet x = 1;\n\nx += `1`;\n```\n\n---\n\n```\nmono line\n```\n"
    );
}

#[test]
fn front_matter_from_title_author_and_core_properties() {
    let core = br#"<?xml version="1.0"?><cp:coreProperties xmlns:cp="x" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Ignored when a Title paragraph exists</dc:title><dc:creator>Ada Lovelace</dc:creator></cp:coreProperties>"#;
    let body = p(Some("Title"), &r("Le rapport : bilan"))
        + &p(Some("Author"), &r("Grace Hopper"))
        + &p(None, &r("Body."));
    let bytes = docx(
        &body,
        &[
            ("word/styles.xml", STYLES.as_bytes()),
            ("docProps/core.xml", core),
        ],
    );
    assert_eq!(
        twin_md(&bytes),
        "---\ntitle: \"Le rapport : bilan\"\nauthor: Grace Hopper\nlang: fr-FR\n---\n\nBody.\n"
    );
    // The twin's front matter is read back as metadata.
    let doc = md2star_rs::reader::read(&twin_md(&bytes));
    assert_eq!(doc.meta.title.as_deref(), Some("Le rapport : bilan"));
    assert_eq!(doc.meta.authors, vec!["Grace Hopper".to_string()]);
}

#[test]
fn unreadable_inputs_are_read_errors() {
    assert!(matches!(
        twin::to_markdown(b"not a zip", SourceKind::Docx, &TwinOptions::default()),
        Err(Error::Read(_))
    ));
    let deck = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/python-reference/with_bib.pptx"),
    )
    .unwrap();
    match twin::to_markdown(&deck, SourceKind::Docx, &TwinOptions::default()) {
        Err(Error::Read(m)) => assert!(m.contains("PowerPoint"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pdf_is_refused_with_the_reason() {
    match twin::to_markdown(b"%PDF-1.7", SourceKind::Pdf, &TwinOptions::default()) {
        Err(Error::Unsupported(m)) => {
            assert!(m.contains("OCR") && m.contains(".docx"), "{m}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn twin_output_is_deterministic() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/python-reference/footnotes_document.docx"),
    )
    .unwrap();
    let a = twin::to_markdown(&bytes, SourceKind::Docx, &TwinOptions::default()).unwrap();
    let b = twin::to_markdown(&bytes, SourceKind::Docx, &TwinOptions::default()).unwrap();
    assert_eq!(a, b);
}
