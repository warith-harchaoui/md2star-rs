//! `twin` for PPTX: hand-built decks (each construction pinned to its PresentationML),
//! the decks md2star (Python) produced, and a round trip through md2star-rs's own
//! PPTX writer.

use std::io::{Cursor, Write};
use std::path::PathBuf;

use md2star_rs::twin::{self, SourceKind, TwinOptions};
use zip::write::SimpleFileOptions;

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0,
    1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xDE,
];

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// One slide: its spTree content, its layout type, extra relationships, notes text.
struct SlideSpec<'a> {
    shapes: String,
    layout: &'a str,
    rels: &'a str,
    notes: Option<&'a str>,
}

fn slide(shapes: String) -> SlideSpec<'static> {
    SlideSpec {
        shapes,
        layout: "obj",
        rels: "",
        notes: None,
    }
}

fn deck(slides: &[SlideSpec], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let o = SimpleFileOptions::default();
        let mut put = |name: &str, data: &[u8]| {
            zip.start_file(name, o).unwrap();
            zip.write_all(data).unwrap();
        };
        let ids: String = (0..slides.len())
            .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 1))
            .collect();
        put(
            "ppt/presentation.xml",
            format!(r#"<?xml version="1.0"?><p:presentation {NS}><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#).as_bytes(),
        );
        let rels: String = (0..slides.len())
            .map(|i| {
                format!(
                    r#"<Relationship Id="rId{}" Type="{REL}/slide" Target="slides/slide{}.xml"/>"#,
                    i + 1,
                    i + 1
                )
            })
            .collect();
        put(
            "ppt/_rels/presentation.xml.rels",
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="{REL_NS}">{rels}</Relationships>"#
            )
            .as_bytes(),
        );
        for (i, s) in slides.iter().enumerate() {
            let n = i + 1;
            put(
                &format!("ppt/slides/slide{n}.xml"),
                format!(r#"<?xml version="1.0"?><p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{}</p:spTree></p:cSld></p:sld>"#, s.shapes).as_bytes(),
            );
            let notes_rel = if s.notes.is_some() {
                format!(
                    r#"<Relationship Id="rIdN" Type="{REL}/notesSlide" Target="../notesSlides/notesSlide{n}.xml"/>"#
                )
            } else {
                String::new()
            };
            put(
                &format!("ppt/slides/_rels/slide{n}.xml.rels"),
                format!(r#"<?xml version="1.0"?><Relationships xmlns="{REL_NS}"><Relationship Id="rIdL" Type="{REL}/slideLayout" Target="../slideLayouts/{}.xml"/>{notes_rel}{}</Relationships>"#, s.layout, s.rels).as_bytes(),
            );
            if let Some(notes) = s.notes {
                put(
                    &format!("ppt/notesSlides/notesSlide{n}.xml"),
                    format!(r#"<?xml version="1.0"?><p:notes {NS}><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image"/><p:cNvSpPr/><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{notes}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#).as_bytes(),
                );
            }
        }
        for layout in ["obj", "secHead", "title", "twoObj"] {
            put(
                &format!("ppt/slideLayouts/{layout}.xml"),
                format!(r#"<?xml version="1.0"?><p:sldLayout {NS} type="{layout}"/>"#).as_bytes(),
            );
        }
        for (name, data) in extra {
            put(name, data);
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn ph(kind: &str, paras: &str) -> String {
    let ph = if kind == "body" {
        r#"<p:ph idx="1"/>"#.to_string()
    } else {
        format!(r#"<p:ph type="{kind}"/>"#)
    };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="{kind}"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{paras}</p:txBody></p:sp>"#
    )
}

fn para(ppr: &str, runs: &str) -> String {
    format!("<a:p>{ppr}{runs}</a:p>")
}

fn run(rpr: &str, text: &str) -> String {
    format!("<a:r><a:rPr {rpr}/><a:t>{text}</a:t></a:r>")
}

fn twin_md(bytes: &[u8]) -> String {
    twin::to_markdown(bytes, SourceKind::Pptx, &TwinOptions::default())
        .unwrap()
        .markdown
}

#[test]
fn slides_titles_bullets_levels_and_numbering() {
    let body = [
        para(r#"<a:pPr><a:buNone/></a:pPr>"#, &run("", "Intro line.")),
        para(r#"<a:pPr lvl="0"/>"#, &run(r#"b="1""#, "bold point")),
        para(r#"<a:pPr lvl="1"/>"#, &run(r#"i="1""#, "sub point")),
        para(
            r#"<a:pPr lvl="0"><a:buAutoNum type="arabicPeriod"/></a:pPr>"#,
            &run("", "first"),
        ),
        para(
            r#"<a:pPr lvl="0"><a:buAutoNum type="arabicPeriod"/></a:pPr>"#,
            &run("", "second"),
        ),
    ]
    .concat();
    let bytes = deck(
        &[
            slide(ph("title", &para("", &run("", "Section")))),
            SlideSpec {
                layout: "secHead",
                ..slide(ph("title", &para("", &run("", "Part One"))))
            },
            slide(ph("title", &para("", &run("", "Agenda"))) + &ph("body", &body)),
        ],
        &[],
    );
    assert_eq!(
        twin_md(&bytes),
        "## Section\n\n---\n\n# Part One\n\n---\n\n## Agenda\n\nIntro line.\n\n- **bold point**\n  - *sub point*\n\n1. first\n2. second\n"
    );
}

#[test]
fn cover_slide_becomes_front_matter_and_notes_become_notes_divs() {
    let cover = ph("ctrTitle", &para("", &run("", "The Deck")))
        + &ph(
            "subTitle",
            &(para("", &run("", "Ada Lovelace")) + &para("", &run("", "5 novembre 2026"))),
        );
    let bytes = deck(
        &[
            slide(cover),
            SlideSpec {
                notes: Some("Say hello first."),
                ..slide(
                    ph("title", &para("", &run("", "Hello")))
                        + &ph("body", &para("", &run("", "World"))),
                )
            },
        ],
        &[],
    );
    assert_eq!(
        twin_md(&bytes),
        "---\ntitle: The Deck\nauthor: Ada Lovelace\ndate: 5 novembre 2026\n---\n\n# Hello\n\n- World\n\n::: notes\nSay hello first.\n:::\n"
    );
}

#[test]
fn tables_pictures_links_code_and_columns() {
    let table = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Table"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Name</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Score</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Ada</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>10</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
    let pic = r#"<p:pic><p:nvPicPr><p:cNvPr id="5" name="Picture" descr="A pixel"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rIdImg"/></p:blipFill><p:spPr/></p:pic>"#;
    let link = para(
        "",
        &(run("", "See ")
            + r#"<a:r><a:rPr><a:hlinkClick r:id="rIdWeb"/></a:rPr><a:t>the site</a:t></a:r>"#),
    );
    let code = para("", &run("", "Code:"))
        + &para(
            "",
            r#"<a:r><a:rPr><a:latin typeface="Courier New"/></a:rPr><a:t>let x = 1;</a:t></a:r>"#,
        )
        + &para(
            "",
            r#"<a:r><a:rPr><a:latin typeface="Courier New"/></a:rPr><a:t>x += 1;</a:t></a:r>"#,
        );
    let textbox = |id: u32, paras: &str| {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="TextBox {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{paras}</p:txBody></p:sp>"#
        )
    };
    let rels = format!(
        r#"<Relationship Id="rIdImg" Type="{REL}/image" Target="../media/image1.png"/><Relationship Id="rIdWeb" Type="{REL}/hyperlink" Target="https://deraison.ai" TargetMode="External"/>"#
    );
    let left = ph("body", &para("", &run("", "Left side")));
    let right = r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="Right"/><p:cNvSpPr/><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>Right side</a:t></a:r></a:p></p:txBody></p:sp>"#;
    let bytes = deck(
        &[
            SlideSpec {
                rels: &rels,
                ..slide(
                    ph("title", &para("", &run("", "Mixed")))
                        + table
                        + pic
                        + &textbox(6, &link)
                        + &textbox(7, &code),
                )
            },
            SlideSpec {
                layout: "twoObj",
                ..slide(ph("title", &para("", &run("", "Two"))) + &left + right)
            },
        ],
        &[("ppt/media/image1.png", PNG)],
    );
    let twin = twin::to_markdown(
        &bytes,
        SourceKind::Pptx,
        &TwinOptions {
            images: true,
            media_dir: "deck_media".into(),
        },
    )
    .unwrap();
    assert_eq!(
        twin.markdown,
        "# Mixed\n\n| Name | Score |\n|---|---|\n| Ada | 10 |\n\n![A pixel](deck_media/image1.png)\n\nSee [the site](https://deraison.ai)\n\nCode:\n\n```\nlet x = 1;\nx += 1;\n```\n\n---\n\n# Two\n\n:::: columns\n::: column\n- Left side\n:::\n\n::: column\n- Right side\n:::\n::::\n"
    );
    assert_eq!(
        twin.media,
        vec![("deck_media/image1.png".to_string(), PNG.to_vec())]
    );
}

#[test]
fn charts_are_reported_not_invented() {
    let chart = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Chart"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></p:graphicFrame>"#;
    let bytes = deck(
        &[slide(ph("title", &para("", &run("", "Data"))) + chart)],
        &[],
    );
    let twin = twin::to_markdown(&bytes, SourceKind::Pptx, &TwinOptions::default()).unwrap();
    assert_eq!(twin.markdown, "# Data\n");
    assert!(twin
        .warnings
        .iter()
        .any(|w| w.code == "twin-unsupported-graphic" && w.message.contains("chart")));
}

#[test]
fn python_reference_deck_with_bibliography() {
    let bytes = std::fs::read(fixtures().join("python-reference/with_bib.pptx")).unwrap();
    assert_eq!(
        twin_md(&bytes),
        "---\ntitle: Bibliography Test\n---\n\nIn the book of (Pearl 2009) we can see that causality is neither correlation nor dependence.\n\n---\n\n# References\n\nPearl, Judea. 2009. *Causality: Models, Reasoning, and Inference, 2nd Edition*. <https://bayes.cs.ucla.edu/BOOK-2K/>.\n"
    );
}

#[test]
fn python_reference_footnotes_come_back_as_footnotes() {
    let bytes = std::fs::read(fixtures().join("python-reference/footnotes_document.pptx")).unwrap();
    let md = twin_md(&bytes);
    let doc = md2star_rs::reader::read(&md);
    let defs = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, md2star_rs::ast::Block::FootnoteDef { .. }))
        .count();
    assert_eq!(defs, 6, "{md}");
    assert!(md.contains("source.[^1]"), "{md}");
    // The notes slide itself is folded into the definitions, not kept as a slide.
    assert!(!md.contains("# Notes"), "{md}");
}

#[cfg(feature = "legacy-pptx")]
#[test]
fn round_trip_through_md2star_rs_pptx() {
    let source = "# First\n\n- a\n- b\n  - c\n\n# Second\n\n1. x\n2. y\n";
    let out = md2star_rs::convert(source, md2star_rs::Format::Pptx, &Default::default()).unwrap();
    let md = twin_md(&out.bytes);
    assert_eq!(
        md,
        "# First\n\n- a\n- b\n  - c\n\n---\n\n# Second\n\n1. x\n2. y\n"
    );
}

#[test]
fn non_decks_are_read_errors() {
    assert!(matches!(
        twin::to_markdown(
            b"PK\x03\x04garbage",
            SourceKind::Pptx,
            &TwinOptions::default()
        ),
        Err(md2star_rs::Error::Read(_))
    ));
    let doc = std::fs::read(fixtures().join("python-reference/footnotes_document.docx")).unwrap();
    match twin::to_markdown(&doc, SourceKind::Pptx, &TwinOptions::default()) {
        Err(md2star_rs::Error::Read(m)) => assert!(m.contains("Word"), "{m}"),
        other => panic!("{other:?}"),
    }
}
