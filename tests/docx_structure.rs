//! The DOCX writer's structure: every part well formed and consistent, the title block,
//! paragraph and character styles, links and bookmarks, footnotes, lists, tables, code,
//! page breaks, the table of contents, and byte-for-byte determinism.
//!
//! Each test opens the produced `.docx` (a zip) and checks the WordprocessingML that
//! Word reads, against the structure Pandoc writes for the same Markdown.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use md2star_rs::{convert, Format, Options, Rendered};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn docx(md: &str, options: &Options) -> Rendered {
    convert(md, Format::Docx, options).expect("converts")
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.insert(f.name().to_string(), data);
    }
    out
}

fn part(bytes: &[u8], name: &str) -> String {
    String::from_utf8(
        parts(bytes)
            .remove(name)
            .unwrap_or_else(|| panic!("no {name}")),
    )
    .unwrap()
}

fn document(md: &str) -> String {
    part(&docx(md, &Options::default()).bytes, "word/document.xml")
}

/// Every attribute value `name="…"` in `xml`.
fn attr_values<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let pat = format!(" {name}=\"");
    xml.match_indices(&pat)
        .map(|(at, _)| {
            let start = at + pat.len();
            let end = xml[start..].find('"').unwrap() + start;
            &xml[start..end]
        })
        .collect()
}

/// The styles of the paragraphs, in order.
fn paragraph_styles(xml: &str) -> Vec<&str> {
    attr_values(xml, "w:val")
        .into_iter()
        .zip(xml.match_indices(" w:val=\""))
        .filter(|(_, (at, _))| xml[..*at].ends_with("<w:pStyle"))
        .map(|(v, _)| v)
        .collect()
}

/// The package is consistent: XML parts parse, every relationship target exists, every
/// `r:id`/`r:embed` resolves, every part has a content type, every style used is defined.
fn assert_consistent(bytes: &[u8]) {
    let parts = parts(bytes);
    for (name, data) in &parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let text = std::str::from_utf8(data).unwrap();
            let mut reader = quick_xml::Reader::from_str(text);
            loop {
                match reader.read_event() {
                    Ok(quick_xml::events::Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{name} is not well formed: {e}"),
                }
            }
        }
    }
    let ct = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    for name in parts.keys().filter(|n| *n != "[Content_Types].xml") {
        let ext = name.rsplit('.').next().unwrap();
        assert!(
            ct.contains(&format!("PartName=\"/{name}\""))
                || ct.contains(&format!("Extension=\"{ext}\"")),
            "{name} has no content type"
        );
    }
    for (name, data) in parts.iter().filter(|(n, _)| n.ends_with(".rels")) {
        let rels = String::from_utf8(data.clone()).unwrap();
        let dir = name.split("_rels/").next().unwrap();
        let ids = attr_values(&rels, "Id");
        let targets = attr_values(&rels, "Target");
        for (target, tag) in targets.iter().zip(rels.split("<Relationship ").skip(1)) {
            if tag.contains("TargetMode=\"External\"") {
                continue;
            }
            let path = if let Some(abs) = target.strip_prefix('/') {
                abs.to_string()
            } else {
                format!("{dir}{target}")
            };
            let path = path.replace("word/../", "");
            assert!(parts.contains_key(&path), "{name} points at missing {path}");
        }
        let source = name
            .replace("_rels/", "")
            .trim_end_matches(".rels")
            .to_string();
        if let Some(src) = parts.get(&source) {
            let src = String::from_utf8_lossy(src);
            for attr in ["r:id", "r:embed"] {
                for id in attr_values(&src, attr) {
                    assert!(
                        ids.contains(&id),
                        "{source}: {attr}={id} has no relationship"
                    );
                }
            }
        }
    }
    let styles = String::from_utf8(parts["word/styles.xml"].clone()).unwrap();
    let defined = attr_values(&styles, "w:styleId");
    for name in ["word/document.xml", "word/footnotes.xml"] {
        let xml = String::from_utf8(parts[name].clone()).unwrap();
        for tag in ["<w:pStyle", "<w:rStyle", "<w:tblStyle"] {
            for (at, _) in xml.match_indices(tag) {
                let rest = &xml[at..];
                let v = attr_values(&rest[..rest.find('>').unwrap()], "w:val")[0];
                assert!(defined.contains(&v), "{name} uses undefined style {v}");
            }
        }
    }
}

#[test]
fn every_fixture_gives_a_consistent_package() {
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let md = std::fs::read_to_string(&path).unwrap();
        let mut options = Options::default();
        options.base_dir = Some(fixtures());
        options.toc = true;
        let out = docx(&md, &options);
        assert_consistent(&out.bytes);
    }
}

#[test]
fn the_title_block_comes_from_the_front_matter() {
    let md = std::fs::read_to_string(fixtures().join("syntax.md")).unwrap();
    let mut options = Options::default();
    options.base_dir = Some(fixtures());
    let out = docx(&md, &options);
    let xml = part(&out.bytes, "word/document.xml");
    let styles = paragraph_styles(&xml);
    assert_eq!(
        &styles[..5],
        &["Title", "Subtitle", "Author", "Author", "Date"],
        "Pandoc's title block"
    );
    assert!(xml.contains(">Every construction md2star reads</w:t>"));
    assert!(xml.contains(">Grace Hopper</w:t>"));
    let core = part(&out.bytes, "docProps/core.xml");
    assert!(core.contains("<dc:title>Every construction md2star reads</dc:title>"));
    assert!(core.contains("<dc:subject>A fixture for the phase 2 ports</dc:subject>"));
    assert!(core.contains("<dc:creator>Ada Lovelace; Grace Hopper</dc:creator>"));
    assert!(core.contains("<cp:keywords>markdown, docx, pptx</cp:keywords>"));
    assert!(core.contains("<dc:language>en-US</dc:language>"));
    assert!(
        !core.contains("dcterms:created"),
        "no timestamp: output is reproducible"
    );
    let styles_xml = part(&out.bytes, "word/styles.xml");
    assert!(styles_xml.contains("<w:lang w:val=\"en-US\"/>"));
}

#[test]
fn without_a_title_the_first_heading_becomes_it_as_in_md2star() {
    let mut options = Options::default();
    options.author = Some("Ada Lovelace, Alan Turing, Grace Hopper".into());
    options.date = Some("21 juin 2026".into());
    let out = docx("# My report\n\nIntro.\n\n# Part one\n\nText.\n", &options);
    let xml = part(&out.bytes, "word/document.xml");
    assert_eq!(
        paragraph_styles(&xml),
        vec![
            "Title",
            "Subtitle",
            "Subtitle",
            "FirstParagraph",
            "Heading1",
            "FirstParagraph"
        ]
    );
    assert!(xml.contains(">Ada Lovelace, Alan Turing and Grace Hopper</w:t>"));
    assert!(xml.contains(">21 juin 2026</w:t>"));
    assert_eq!(
        xml.matches(">My report</w:t>").count(),
        1,
        "promoted, not repeated"
    );
    assert!(part(&out.bytes, "docProps/core.xml").contains("<dc:title>My report</dc:title>"));
}

#[test]
fn an_abstract_follows_the_title() {
    let xml = document("---\ntitle: T\nabstract: A *short* summary.\n---\n\nBody.\n");
    assert_eq!(
        paragraph_styles(&xml),
        vec!["Title", "AbstractTitle", "Abstract", "FirstParagraph"]
    );
    assert!(xml.contains(">Abstract</w:t>"));
}

#[test]
fn paragraphs_after_a_heading_open_a_section() {
    let xml = document("## Section\n\nOne.\n\nTwo.\n\n> Quoted.\n\nThree.\n");
    assert_eq!(
        paragraph_styles(&xml),
        vec![
            "Heading2",
            "FirstParagraph",
            "BodyText",
            "BlockText",
            "BodyText"
        ]
    );
}

#[test]
fn character_formatting() {
    let xml = document("**b** *i* ~~s~~ H~2~O 2^10^ `code`\n");
    assert!(xml.contains("<w:rPr><w:b/><w:bCs/></w:rPr><w:t xml:space=\"preserve\">b</w:t>"));
    assert!(xml.contains("<w:rPr><w:i/><w:iCs/></w:rPr><w:t xml:space=\"preserve\">i</w:t>"));
    assert!(xml.contains("<w:rPr><w:strike/></w:rPr><w:t xml:space=\"preserve\">s</w:t>"));
    assert!(xml
        .contains("<w:vertAlign w:val=\"subscript\"/></w:rPr><w:t xml:space=\"preserve\">2</w:t>"));
    assert!(xml.contains(
        "<w:vertAlign w:val=\"superscript\"/></w:rPr><w:t xml:space=\"preserve\">10</w:t>"
    ));
    assert!(xml.contains(
        "<w:rStyle w:val=\"VerbatimChar\"/></w:rPr><w:t xml:space=\"preserve\">code</w:t>"
    ));
}

#[test]
fn links_are_real_hyperlinks_and_anchors_hit_bookmarks() {
    let out = docx(
        "# Intro {#intro}\n\nSee [the site](https://deraison.ai \"home\") and [above](#intro).\n",
        &Options::default(),
    );
    let xml = part(&out.bytes, "word/document.xml");
    let rels = part(&out.bytes, "word/_rels/document.xml.rels");
    let rid = attr_values(&xml, "r:id")[0];
    assert!(rels.contains(&format!(
        "Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://deraison.ai\" TargetMode=\"External\""
    )));
    assert!(xml.contains("w:tooltip=\"home\""));
    assert!(xml.contains(
        "<w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t xml:space=\"preserve\">the site</w:t>"
    ));
    assert!(xml.contains("<w:hyperlink w:anchor=\"intro\""));
    // The heading is the title here (md2star), so the bookmark is on a later heading:
    let xml = document("# T\n\n## Intro {#intro}\n\n[up](#intro)\n");
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"intro\"/>"));
    assert!(xml.contains("<w:bookmarkEnd w:id=\"1\"/>"));
}

#[test]
fn footnotes_are_word_footnotes_everywhere() {
    let out = docx(
        "Text[^a] and inline^[In place.].\n\n| x |\n|---|\n| cell[^b] |\n\n[^a]: First.\n\n[^b]: In a table.\n",
        &Options::default(),
    );
    let xml = part(&out.bytes, "word/document.xml");
    let notes = part(&out.bytes, "word/footnotes.xml");
    assert_eq!(xml.matches("<w:footnoteReference w:id=").count(), 3);
    assert!(notes.contains("<w:footnote w:type=\"separator\" w:id=\"-1\">"));
    assert!(notes.contains("<w:footnote w:type=\"continuationSeparator\" w:id=\"0\">"));
    for (id, text) in [(1, "First."), (2, "In place."), (3, "In a table.")] {
        let start = notes.find(&format!("<w:footnote w:id=\"{id}\">")).unwrap();
        let note = &notes[start..];
        let note = &note[..note.find("</w:footnote>").unwrap()];
        assert!(
            note.contains("<w:pStyle w:val=\"FootnoteText\"/>"),
            "{note}"
        );
        assert!(note.contains("<w:footnoteRef/>"), "{note}");
        assert!(note.contains(text), "{note}");
    }
}

#[test]
fn lists_use_native_numbering() {
    let out = docx(
        "- [x] done\n- [ ] open\n- plain\n  1. nested\n\n3. three\n4. four\n",
        &Options::default(),
    );
    let xml = part(&out.bytes, "word/document.xml");
    let numbering = part(&out.bytes, "word/numbering.xml");
    assert!(xml.contains(">☒ </w:t>") && xml.contains(">☐ </w:t>"));
    assert!(xml.contains("<w:ilvl w:val=\"1\"/>"), "nested level");
    assert!(paragraph_styles(&xml).iter().all(|s| *s == "Compact"));
    // The list starting at 3 has a definition starting at 3.
    assert!(numbering.contains("<w:start w:val=\"3\"/>"));
    // Every numId used is declared once, and points at a declared definition.
    for id in attr_values(&xml, "w:val")
        .iter()
        .zip(xml.match_indices(" w:val=\""))
        .filter(|(_, (at, _))| xml[..*at].ends_with("<w:numId"))
        .map(|(v, _)| *v)
    {
        assert_eq!(
            numbering
                .matches(&format!("<w:num w:numId=\"{id}\">"))
                .count(),
            1
        );
    }
    let bullets = numbering.find("<w:numFmt w:val=\"bullet\"/>").unwrap();
    assert!(
        bullets < numbering.find("<w:num ").unwrap(),
        "definitions come first"
    );
}

#[test]
fn tables_keep_alignment_header_and_caption() {
    let xml = document(
        "| L | C | R | D |\n|:--|:-:|--:|---|\n| a | b | c | d |\n\nTable: The caption {#tbl:x}\n",
    );
    let caption = xml.find(">The caption</w:t>").unwrap();
    let table = xml.find("<w:tbl>").unwrap();
    assert!(caption < table, "caption before the table, as Pandoc");
    assert!(xml.contains(
        "<w:pStyle w:val=\"TableCaption\"/></w:pPr><w:bookmarkStart w:id=\"1\" w:name=\"tbl:x\"/>"
    ));
    assert!(xml.contains("<w:tblStyle w:val=\"MyTable\"/>"));
    assert!(xml.contains("<w:tblCaption w:val=\"The caption\"/>"));
    assert!(xml.contains("<w:trPr><w:tblHeader/></w:trPr>"));
    assert_eq!(xml.matches("<w:jc w:val=\"left\"/>").count(), 2);
    assert_eq!(xml.matches("<w:jc w:val=\"center\"/>").count(), 2);
    assert_eq!(xml.matches("<w:jc w:val=\"right\"/>").count(), 2);
    assert_eq!(xml.matches("<w:gridCol ").count(), 4);
    // md2star's small style from six columns.
    let wide = document("|a|b|c|d|e|f|\n|-|-|-|-|-|-|\n|1|2|3|4|5|6|\n");
    assert!(wide.contains("<w:tblStyle w:val=\"MyTableSmall\"/>"));
}

#[test]
fn code_blocks_mermaid_and_page_breaks() {
    let out = docx(
        "```rust\nfn main() {\n    x\n}\n```\n\n---\n\n```mermaid\ngraph LR\n```\n",
        &Options::default(),
    );
    let xml = part(&out.bytes, "word/document.xml");
    assert!(xml.contains("<w:pStyle w:val=\"SourceCode\"/>"));
    assert_eq!(
        xml.matches("<w:r><w:br/></w:r>").count(),
        2,
        "one paragraph, line breaks"
    );
    assert!(
        xml.contains("<w:t xml:space=\"preserve\">    x</w:t>"),
        "indentation kept"
    );
    assert!(
        xml.contains("<w:br w:type=\"page\"/>"),
        "--- is a page break (md2star)"
    );
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "mermaid-not-rendered"));
}

#[test]
fn the_table_of_contents_is_a_field_word_rebuilds() {
    let mut options = Options::default();
    options.toc = true;
    let out = docx(
        "# Doc\n\n## One\n\n### One.a\n\n#### Too deep\n\n## Two\n",
        &options,
    );
    let xml = part(&out.bytes, "word/document.xml");
    assert!(xml.contains("<w:docPartGallery w:val=\"Table of Contents\"/>"));
    assert!(xml.contains("<w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/>"));
    assert!(xml.contains(
        "<w:instrText xml:space=\"preserve\"> TOC \\o \"1-3\" \\h \\z \\u </w:instrText>"
    ));
    assert_eq!(xml.matches("<w:fldChar w:fldCharType=\"end\"/>").count(), 1);
    // Pre-filled entries, linked to the headings' bookmarks; the title is not one.
    // (The first entry also opens the field.)
    assert!(xml.contains("<w:pStyle w:val=\"TOC2\"/></w:pPr><w:r><w:fldChar"));
    assert!(xml.contains("<w:hyperlink w:anchor=\"one\" w:history=\"1\">"));
    assert!(xml.contains("<w:pStyle w:val=\"TOC3\"/></w:pPr><w:hyperlink w:anchor=\"one.a\""));
    assert!(!xml.contains("w:anchor=\"too-deep\""));
    assert!(xml.contains("w:name=\"one\""));
    // `toc: true` in the front matter works as well, `toc-depth` and `toc-title` too.
    let xml = document("---\ntoc: true\ntoc-depth: 1\ntoc-title: Sommaire\n---\n\n# A\n\n## B\n");
    assert!(xml.contains("\\o \"1-1\""));
    assert!(xml.contains(">Sommaire</w:t>"));
}

#[test]
fn a_custom_style_div_gets_its_style() {
    let out = docx(
        "::: {custom-style=\"Warning Box\"}\nCareful.\n:::\n",
        &Options::default(),
    );
    let xml = part(&out.bytes, "word/document.xml");
    assert!(xml.contains("<w:pStyle w:val=\"WarningBox\"/>"));
    let styles = part(&out.bytes, "word/styles.xml");
    assert!(styles.contains(
        "w:styleId=\"WarningBox\"><w:name w:val=\"Warning Box\"/><w:basedOn w:val=\"BodyText\"/>"
    ));
    assert_consistent(&out.bytes);
}

#[test]
fn conversion_is_byte_identical_run_after_run_and_across_threads() {
    let md = std::fs::read_to_string(fixtures().join("syntax.md")).unwrap();
    let mut options = Options::default();
    options.base_dir = Some(fixtures());
    options.toc = true;
    let first = docx(&md, &options).bytes;
    assert_eq!(first, docx(&md, &options).bytes);
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let md = md.clone();
            let options = options.clone();
            std::thread::spawn(move || docx(&md, &options).bytes)
        })
        .collect();
    for h in handles {
        assert_eq!(
            h.join().unwrap(),
            first,
            "not deterministic under concurrency"
        );
    }
}

#[cfg(feature = "cli")]
#[test]
fn the_cli_writes_a_docx() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.docx");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_md2star-rs"))
        .args(["docx", fixtures().join("syntax.md").to_str().unwrap(), "-o"])
        .arg(&out)
        .arg("-q")
        .status()
        .unwrap();
    assert!(status.success());
    assert_consistent(&std::fs::read(&out).unwrap());
}
