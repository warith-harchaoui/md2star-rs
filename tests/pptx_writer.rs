//! The pure-Rust PresentationML writer, opened back up: every test reads the produced
//! zip and checks the exact OOXML it promises — layouts instantiated from the template,
//! placeholders by type and index, `a:tbl`, hyperlinks as relationships, notes slides,
//! pictures with their alt text, sizes never under `--min-pt`.

mod pptx_common;

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

fn deck_md() -> String {
    std::fs::read_to_string(fixtures().join("deck.md")).unwrap()
}

#[test]
fn deck_md_lands_on_the_expected_layouts() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert_eq!(
        deck.layout_names(),
        vec![
            "Title Slide",
            "Section Header",
            "Title Only",
            "Two Content",
            "Title Only",
            "Title Only",
            "Panoramic Picture with Caption",
            "Title and Content",
            "Title and Content",
            "Title Slide",
        ]
    );
    // The explicit hint is the one md2pptx --smart-layout also honours.
    assert_eq!(deck.layout_names()[3], "Two Content");
}

#[test]
fn the_cover_keeps_its_subtitle_and_the_front_matter_byline() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let s1 = deck.slide(1);
    assert!(s1.contains("<p:ph type=\"ctrTitle\"/>"), "{s1}");
    assert!(s1.contains(">Q3 review<"));
    assert!(s1.contains("The cover keeps its own subtitle."));
    assert!(s1.contains("What the numbers say"));
    assert!(s1.contains("Warith Harchaoui · 2026-11-05"));
}

#[test]
fn notes_go_to_notes_slides_and_never_on_the_slide() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert!(!deck.slide(1).contains("Welcome everyone."));
    let notes = deck.notes(1).expect("slide 1 has notes");
    assert!(notes.contains("Welcome everyone."));
    assert!(notes.contains("<p:ph type=\"body\" idx=\"1\"/>"));
    // Notes written after the slide's content stay with that slide (slide 9).
    assert!(deck.notes(9).unwrap().contains("Notes written after"));
    assert!(deck.notes(2).is_none());
    // The template has no notes master: one is added, and declared.
    assert!(deck.part("ppt/notesMasters/notesMaster1.xml").is_some());
    assert!(deck
        .part("ppt/presentation.xml")
        .unwrap()
        .contains("notesMasterIdLst"));
    assert!(deck
        .part("[Content_Types].xml")
        .unwrap()
        .contains("notesSlide+xml"));
}

#[test]
fn placeholders_are_instantiated_by_type_and_index_without_ghosts() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let two = deck.slide(4);
    assert!(two.contains("<p:ph type=\"title\"/>"));
    assert!(
        two.contains("<p:ph idx=\"1\"/>"),
        "left column in idx 1: {two}"
    );
    assert!(two.contains("New markets") && two.contains("Pricing"));
    // The picture of the right column, with its alt text as description.
    assert!(two.contains("descr=\"Trend\""));
    // No date / footer / slide-number placeholder is emitted empty.
    for n in 1..=deck.slide_count() {
        let x = deck.slide(n);
        assert!(!x.contains("type=\"dt\"") && !x.contains("type=\"sldNum\""));
    }
}

#[test]
fn tables_are_real_tables() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let s = deck.slide(8);
    assert!(s.contains("<a:tbl>"));
    assert_eq!(s.matches("<a:gridCol ").count(), 3);
    assert_eq!(s.matches("<a:tr ").count(), 3);
    assert!(s.contains("<a:tableStyleId>{"));
    assert!(s.contains("algn=\"r\""), "right-aligned numbers: {s}");
    assert!(s.contains(">Region<") && s.contains(">17<"));
}

#[test]
fn inline_formatting_and_real_hyperlinks() {
    let md =
        "# Links\n\nSee **bold**, *italic*, `code` and [the site](https://example.com/a?b=1).\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let deck = Deck::open(&out.bytes);
    let s = deck.slide(1);
    assert!(s.contains("b=\"1\""));
    assert!(s.contains("i=\"1\""));
    assert!(s.contains("<a:latin typeface=\"Consolas\"/>"));
    assert!(s.contains("<a:hlinkClick r:id=\"rId2\"/>"), "{s}");
    let rels = deck.part("ppt/slides/_rels/slide1.xml.rels").unwrap();
    assert!(rels.contains("Target=\"https://example.com/a?b=1\" TargetMode=\"External\""));
    assert!(rels.contains("relationships/hyperlink"));
}

#[test]
fn lists_keep_levels_numbers_and_tasks() {
    let md = "# L\n\n- a\n  - b\n\n3. x\n4. y\n\n- [x] done\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let s = Deck::open(&out.bytes).slide(1);
    assert!(s.contains("lvl=\"1\""), "{s}");
    assert!(s.contains("<a:buAutoNum type=\"arabicPeriod\" startAt=\"3\"/>"));
    assert!(s.contains("startAt=\"4\""));
    assert!(s.contains("☑ "));
}

#[test]
fn code_is_a_monospaced_panel_with_every_line() {
    let md = "# Code\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let s = Deck::open(&out.bytes).slide(1);
    assert!(s.contains("prst=\"roundRect\""));
    assert!(s.contains("<a:srgbClr val=\"141416\"/>"));
    assert!(s.contains(">fn main() {<"));
    assert!(
        s.contains(">    println!(&quot;hi&quot;);<"),
        "indent kept: {s}"
    );
    assert!(s.contains("<a:latin typeface=\"Consolas\"/>"));
}

#[test]
fn images_are_embedded_with_alt_text_and_svg_keeps_a_fallback() {
    let md = "# Pic\n\n![A photo](images/photo-8x6.jpg)\n\n# Vector\n\n![A diagram](images/diagram.svg)\n";
    let out = convert(md, Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert!(deck.slide(1).contains("descr=\"A photo\""));
    assert!(deck.media().iter().any(|m| m.ends_with(".jpeg")));
    let svg = deck.slide(2);
    assert!(svg.contains("svgBlip"));
    assert!(deck.media().iter().any(|m| m.ends_with(".svg")));
    assert!(deck.media().iter().any(|m| m.ends_with(".png")));
    let ct = deck.part("[Content_Types].xml").unwrap();
    assert!(ct.contains("Extension=\"svg\""));
    assert!(ct.contains("Extension=\"jpeg\""));
    // A picture stays inside the slide.
    let (w, h) = deck.slide_size();
    for (x, y, cx, cy) in deck.picture_frames(1) {
        assert!(x >= 0 && y >= 0 && x + cx <= w && y + cy <= h);
    }
}

#[test]
fn a_missing_image_is_a_warning_and_its_alt_text() {
    let md = "# P\n\n![Gone](images/nope.png)\n";
    let out = convert(md, Format::Pptx, &options()).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "image-not-loaded"));
    assert!(Deck::open(&out.bytes).slide(1).contains(">Gone<"));
}

#[test]
fn a_photo_credit_prints_the_site_and_links_the_page() {
    let md = "# Photo\n\n![A street](images/photo-8x6.jpg \"Photo: Ada — https://www.unsplash.com/photos/abc\")\n";
    let out = convert(md, Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let s = deck.slide(1);
    assert!(s.contains("name=\"md2star-credit\""), "{s}");
    assert!(s.contains(">Photo: Ada — unsplash.com<"));
    let rels = deck.part("ppt/slides/_rels/slide1.xml.rels").unwrap();
    assert!(rels.contains("https://www.unsplash.com/photos/abc"));
    let report = audit(&out.bytes, &AuditOptions::default()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn footnotes_become_superscripts_and_a_notes_slide() {
    let md = "# A\n\nClaim.[^1] Aside.^[Inline note.]\n\n[^1]: The source.\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    let deck = Deck::open(&out.bytes);
    assert_eq!(deck.slide_count(), 2);
    assert!(deck.slide(1).contains("baseline=\"30000\""));
    let notes = deck.slide(2);
    assert!(notes.contains(">Notes<"));
    assert!(notes.contains("The source.") && notes.contains("Inline note."));
}

#[test]
fn an_unknown_layout_hint_warns_and_falls_back() {
    let md = "# A {layout=\"No Such Layout\"}\n\n- x\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "layout-hint-unknown"));
    assert_eq!(
        Deck::open(&out.bytes).layout_names(),
        vec!["Title and Content"]
    );
}

#[test]
fn a_hint_matching_only_by_case_is_honoured_and_said() {
    let md = "# A {layout=\"two content\"}\n\n- x\n";
    let out = convert(md, Format::Pptx, &Options::default()).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "layout-hint-case"));
    assert_eq!(Deck::open(&out.bytes).layout_names(), vec!["Two Content"]);
}

#[test]
fn a_reference_template_supplies_its_own_layouts() {
    let template = std::fs::read(fixtures().join("templates/Presentation1.pptx")).unwrap();
    let mut options = options();
    options.reference_doc = Some(template);
    let out = convert(&deck_md(), Format::Pptx, &options).unwrap();
    let deck = Deck::open(&out.bytes);
    let info = md2star_rs::pptx::template::inspect(
        &std::fs::read(fixtures().join("templates/Presentation1.pptx")).unwrap(),
    )
    .unwrap();
    for name in deck.layout_names() {
        assert!(
            info.layout(&name).is_some(),
            "{name} comes from the template"
        );
    }
    // The template's example slide is gone, its masters and theme are kept.
    assert_eq!(deck.slide_count(), 10);
    let report = audit(&out.bytes, &AuditOptions::default()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn a_bad_reference_template_is_a_template_error() {
    let mut options = Options::default();
    options.reference_doc = Some(b"not a deck".to_vec());
    assert!(matches!(
        convert("# A", Format::Pptx, &options),
        Err(md2star_rs::Error::Template(_))
    ));
}

#[test]
fn core_properties_are_fixed_and_carry_the_title() {
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    let core = deck.part("docProps/core.xml").unwrap();
    assert!(core.contains("<dc:title>Q3 review</dc:title>"));
    assert!(core.contains("<dc:creator>Warith Harchaoui</dc:creator>"));
    assert!(core.contains("2001-01-01T00:00:00Z"));
    assert!(deck.part("docProps/thumbnail.jpeg").is_none());
    assert!(!deck.part("_rels/.rels").unwrap().contains("thumbnail"));
}

#[test]
fn the_package_is_consistent() {
    // Every part a relationship targets exists, every part has a content type.
    let out = convert(&deck_md(), Format::Pptx, &options()).unwrap();
    let deck = Deck::open(&out.bytes);
    deck.assert_consistent();
}

#[test]
fn every_byte_is_the_same_on_every_run() {
    let a = convert(&deck_md(), Format::Pptx, &options()).unwrap().bytes;
    let b = convert(&deck_md(), Format::Pptx, &options()).unwrap().bytes;
    assert_eq!(a, b);
    let mut min = options();
    min.min_pt = Some(30.0);
    let c = convert(&deck_md(), Format::Pptx, &min).unwrap().bytes;
    let d = convert(&deck_md(), Format::Pptx, &min).unwrap().bytes;
    assert_eq!(c, d);
}
