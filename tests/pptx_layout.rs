//! Layout selection and `--min-pt`, read back from the produced decks: roles land on
//! the layouts a template names for them, the fit model splits rather than shrinks,
//! and nothing the Markdown says is dropped.

mod pptx_common;

use std::io::{Cursor, Read, Write};

use md2star_rs::audit::{audit, AuditOptions};
use md2star_rs::{convert, Format, Options};
use pptx_common::*;

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn options() -> Options {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o
}

/// The bundled template with five layouts renamed after md2star's roles.
fn role_named_template() -> Vec<u8> {
    let renames = [
        ("Name Card", "Big Number"),
        ("Title Only", "Statement"),
        ("Quote with Caption", "Quote"),
        ("Content with Caption", "Recap"),
        ("Picture with Caption", "Closing"),
    ];
    let mut zin = zip::ZipArchive::new(Cursor::new(md2star_rs::templates::DEFAULT_PPTX)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name.starts_with("ppt/slideLayouts/slideLayout") {
            let mut s = String::from_utf8(data).unwrap();
            for (a, b) in renames {
                s = s.replace(
                    &format!("<p:cSld name=\"{a}\""),
                    &format!("<p:cSld name=\"{b}\""),
                );
            }
            data = s.into_bytes();
        }
        out.start_file(name, opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn roles_land_on_the_layouts_the_template_names_like_md2star_does() {
    // The same ten layouts `md2pptx --smart-layout --template` picks on this template
    // (checked against md2star 3.12): hints and roles decide, no model is asked.
    let mut o = options();
    o.reference_doc = Some(role_named_template());
    let md = std::fs::read_to_string(fixtures().join("deck.md")).unwrap();
    let out = convert(&md, Format::Pptx, &o).unwrap();
    let deck = Deck::open(&out.bytes);
    assert_eq!(
        deck.layout_names(),
        vec![
            "Title Slide",
            "Section Header",
            "Big Number",
            "Two Content",
            "Statement",
            "Quote",
            "Panoramic Picture with Caption",
            "Title and Content",
            "Recap",
            "Closing",
        ]
    );
    // A statement layout with no body box still shows its sentence.
    assert!(deck.slide(5).contains("Simplicity scales."));
    assert!(deck.slide(3).contains("+18 %"));
    let report = audit(&out.bytes, &AuditOptions::default()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn a_statement_without_its_layout_is_set_large_on_the_stage() {
    let out = convert(
        "# A\n\n## Big idea\n\nLess is more.\n\n## B\n\n- x\n",
        Format::Pptx,
        &Options::default(),
    )
    .unwrap();
    let deck = Deck::open(&out.bytes);
    assert_eq!(deck.layout_names()[1], "Title Only");
    let s = deck.slide(2);
    assert!(s.contains("algn=\"ctr\""));
    assert!(
        deck.sizes(2).iter().any(|&z| z >= 28.0),
        "{:?}",
        deck.sizes(2)
    );
}

#[test]
fn min_pt_sets_nothing_under_the_floor_and_the_audit_agrees() {
    let md = std::fs::read_to_string(fixtures().join("deck.md")).unwrap();
    let mut o = options();
    o.min_pt = Some(30.0);
    let out = convert(&md, Format::Pptx, &o).unwrap();
    let deck = Deck::open(&out.bytes);
    for n in 1..=deck.slide_count() {
        for z in deck.sizes(n) {
            assert!(z >= 30.0, "slide {n}: {z} pt under the floor");
        }
        // Text left to the template must not be under the floor either: no
        // shrink-on-overflow, and every placeholder size raised explicitly.
        assert!(!deck.slide(n).contains("normAutofit"));
    }
    let report = audit(
        &out.bytes,
        &AuditOptions {
            min_pt: Some(30.0),
            strict: true,
        },
    )
    .unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn a_slide_no_layout_holds_is_split_with_a_continuation_label() {
    let items: String = (1..=24)
        .map(|i| format!("- Point number {i} says something worth a whole line\n"))
        .collect();
    let md = format!("# Many points\n\n{items}");
    let mut o = Options::default();
    o.min_pt = Some(30.0);
    let out = convert(&md, Format::Pptx, &o).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "slide-split"));
    let deck = Deck::open(&out.bytes);
    assert!(deck.slide_count() > 2);
    assert!(deck.slide(2).contains("Many points (cont.)"));
    // Every point is on some slide, in order, once.
    let all: String = (1..=deck.slide_count()).map(|n| deck.slide(n)).collect();
    let mut last = 0;
    for i in 1..=24 {
        let at = all.find(&format!("Point number {i} ")).expect("kept");
        assert!(at > last || i == 1);
        last = at;
    }
    let report = audit(
        &out.bytes,
        &AuditOptions {
            min_pt: Some(30.0),
            strict: true,
        },
    )
    .unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn a_french_deck_continues_with_suite_and_a_table_repeats_its_header() {
    let rows: String = (1..=30).map(|i| format!("| ligne {i} | {i} |\n")).collect();
    let md =
        format!("---\nlang: fr-FR\n---\n\n## Chiffres\n\n| Nom | Valeur |\n|---|---:|\n{rows}");
    let mut o = Options::default();
    o.min_pt = Some(30.0);
    let out = convert(&md, Format::Pptx, &o).unwrap();
    let deck = Deck::open(&out.bytes);
    assert!(deck.slide_count() >= 2);
    // The label may sit on a line of its own in a narrow title box.
    assert!(deck.slide(2).contains(">Chiffres<") || deck.slide(2).contains("Chiffres (suite)"));
    assert!(deck.slide(2).contains("(suite)"));
    for n in 1..=deck.slide_count() {
        assert!(
            deck.slide(n).contains(">Nom<"),
            "slide {n} repeats the header"
        );
    }
    let joined: String = (1..=deck.slide_count()).map(|n| deck.slide(n)).collect();
    assert!(joined.contains(">ligne 30<"));
}

#[test]
fn a_long_listing_splits_between_lines() {
    let code: String = (1..=60).map(|i| format!("let x{i} = {i};\n")).collect();
    let md = format!("## Code\n\n```rust\n{code}```\n");
    let out = convert(&md, Format::Pptx, &Options::default()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert!(deck.slide_count() >= 2);
    let joined: String = (1..=deck.slide_count()).map(|n| deck.slide(n)).collect();
    assert!(joined.contains(">let x1 = 1;<") && joined.contains(">let x60 = 60;<"));
    let report = audit(&out.bytes, &AuditOptions::default()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn two_columns_go_to_a_two_box_layout() {
    let md = "## Compare\n\n:::: columns\n::: column\n- left a\n- left b\n:::\n::: column\n- right a\n:::\n::::\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert_eq!(deck.layout_names(), vec!["Two Content"]);
    let s = deck.slide(1);
    assert!(s.contains("<p:ph idx=\"1\"/>") && s.contains("<p:ph idx=\"2\"/>"));
    assert!(s.find("left a").unwrap() < s.find("right a").unwrap());
}

#[test]
fn a_lone_figure_goes_where_it_comes_out_largest() {
    let md = "## The chart\n\n![Quarterly revenue](images/photo-8x6.jpg)\n";
    let out = convert(md, Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let (w, h) = deck.slide_size();
    let frames = deck.picture_frames(1);
    assert_eq!(frames.len(), 1);
    let share = (frames[0].2 * frames[0].3) as f64 / (w * h) as f64;
    assert!(share > 0.2, "the picture covers {share:.2} of the slide");
}

#[test]
fn a_section_after_the_cover_is_a_divider() {
    let md = "# Deck\n\n# Part one\n\n## Inside\n\n- a\n\n# Part two\n\n## More\n\n- b\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let names = Deck::open(&out.bytes).layout_names();
    assert_eq!(names[0], "Title Slide");
    assert_eq!(names[1], "Section Header");
    assert_eq!(names[3], "Section Header");
}

/// Every word of the Markdown's text reaches the deck (slides or notes).
#[test]
fn nothing_is_dropped_from_the_fixtures() {
    for name in [
        "deck",
        "syntax",
        "comprehensive_presentation",
        "branded_slides",
        "simple",
    ] {
        let md = std::fs::read_to_string(fixtures().join(format!("{name}.md"))).unwrap();
        for min in [None, Some(30.0)] {
            let mut o = options();
            o.min_pt = min;
            let out = convert(&md, Format::Pptx, &o).unwrap();
            let deck = Deck::open(&out.bytes);
            let mut text = String::new();
            for n in 1..=deck.slide_count() {
                text.push_str(&deck.slide(n));
                if let Some(notes) = deck.notes(n) {
                    text.push_str(&notes);
                }
            }
            let visible = strip_tags(&text);
            let doc = md2star_rs::reader::read(&md);
            for word in words_of(&doc) {
                assert!(
                    visible.contains(&word),
                    "{name} (min {min:?}): `{word}` was dropped"
                );
            }
        }
    }
}

fn strip_tags(xml: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in xml.chars() {
        match c {
            '<' => {
                inside = true;
                out.push(' ');
            }
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn words_of(doc: &md2star_rs::Document) -> Vec<String> {
    use md2star_rs::ast::{plain_text, Block};
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            match b {
                Block::Paragraph(i) | Block::Heading { content: i, .. } => {
                    out.extend(plain_text(i).split_whitespace().map(str::to_string))
                }
                Block::BlockQuote(x) | Block::Div { content: x, .. } | Block::Notes(x) => {
                    walk(x, out)
                }
                Block::List { items, .. } => {
                    for it in items {
                        walk(&it.content, out);
                    }
                }
                Block::CodeBlock { code, .. } | Block::Mermaid { code, .. } => {
                    out.extend(code.split_whitespace().map(str::to_string))
                }
                Block::Table { headers, rows, .. } => {
                    for c in headers.iter().chain(rows.iter().flatten()) {
                        out.extend(plain_text(c).split_whitespace().map(str::to_string));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out);
    // Words long enough to be specific; punctuation-only tokens say nothing.
    // A citation key is replaced by its rendered form once the bibliography resolves.
    out.into_iter()
        .filter(|w| !w.starts_with('@') && !w.starts_with("[@") && !w.contains("@"))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.chars().count() >= 4)
        .collect()
}
