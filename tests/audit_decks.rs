//! `audit` on real decks: every fixture md2star-rs renders comes out clean (with and
//! without `--min-pt 30`), a deck broken on purpose reports each defect under
//! md2star's own kind, and Pandoc-made decks get the findings `md2star audit` gives.

mod pptx_common;

use std::io::{Cursor, Read, Write};

use md2star_rs::audit::{audit, AuditOptions, FindingKind};
use md2star_rs::{convert, Format, Options};

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn every_fixture_deck_is_clean() {
    let mut names: Vec<_> = std::fs::read_dir(fixtures())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    names.sort();
    assert!(names.len() >= 10);
    for path in names {
        let md = std::fs::read_to_string(&path).unwrap();
        for min in [None, Some(30.0)] {
            let mut o = Options::default();
            o.base_dir = Some(fixtures());
            o.min_pt = min;
            let out = convert(&md, Format::Pptx, &o).unwrap();
            let report = audit(
                &out.bytes,
                &AuditOptions {
                    min_pt: min,
                    strict: true,
                },
            )
            .unwrap();
            assert!(
                report.findings.is_empty(),
                "{} (min {min:?}): {:#?}",
                path.display(),
                report.findings
            );
            assert!(report.slides > 0);
        }
    }
}

/// Rewrite one part of a deck.
fn patch(deck: &[u8], part: &str, edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(Cursor::new(deck)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == part {
            data = edit(String::from_utf8(data).unwrap()).into_bytes();
        }
        out.start_file(name, opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn text_box(id: u32, name: &str, frame: (i64, i64, i64, i64), sz: u32, text: &str) -> String {
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"{sz}\"/><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>",
        frame.0, frame.1, frame.2, frame.3
    )
}

#[test]
fn a_broken_deck_reports_each_defect_by_name() {
    let clean = convert(
        "# One\n\n- a\n\n# Two\n\n- b\n",
        Format::Pptx,
        &Options::default(),
    )
    .unwrap()
    .bytes;
    let broken = patch(&clean, "ppt/slides/slide1.xml", |xml| {
        let extra = [
            // Half off the right edge.
            text_box(40, "Runaway", (11_000_000, 1_000_000, 3_000_000, 500_000), 2400, "off"),
            // Two boxes on top of each other.
            text_box(41, "Left", (1_000_000, 3_000_000, 3_000_000, 1_000_000), 2400, "one thing"),
            text_box(42, "Right", (1_500_000, 3_200_000, 3_000_000, 1_000_000), 2400, "another"),
            // Eight-point text.
            text_box(43, "Tiny", (1_000_000, 5_000_000, 3_000_000, 400_000), 800, "small print"),
            // An empty body placeholder.
            "<p:sp><p:nvSpPr><p:cNvPr id=\"44\" name=\"Ghost\"/><p:cNvSpPr/><p:nvPr><p:ph idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>".to_string(),
            // A box far too small for its text.
            text_box(45, "Crammed", (6_000_000, 5_000_000, 1_000_000, 200_000), 2400, &"word ".repeat(40)),
        ]
        .concat();
        xml.replace("</p:spTree>", &format!("{extra}</p:spTree>"))
    });
    let report = audit(&broken, &AuditOptions::default()).unwrap();
    let kinds: Vec<(FindingKind, String)> = report
        .findings
        .iter()
        .map(|f| (f.kind, f.shape.clone()))
        .collect();
    assert!(
        kinds.contains(&(FindingKind::OffCanvas, "Runaway".into())),
        "{kinds:?}"
    );
    assert!(
        kinds.iter().any(|(k, _)| *k == FindingKind::Overlap),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(FindingKind::GhostPlaceholder, "Ghost".into())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(FindingKind::Unreadable, "Tiny".into())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(FindingKind::Overrun, "Crammed".into())),
        "{kinds:?}"
    );
    assert!(report.fails(false));
    // Severity order within the slide: off-slide first, overrun last.
    assert_eq!(report.findings[0].kind, FindingKind::OffCanvas);
    assert_eq!(report.findings.last().unwrap().kind, FindingKind::Overrun);
    let overrun = report.findings.last().unwrap();
    assert_eq!(overrun.measured_pt, Some(24.0));
    assert!(overrun.fits_pt.unwrap() < 24.0);
    // With --min-pt 30 the 24 pt boxes are under the floor too; the slide number
    // band would be exempt, these are not.
    let floor = audit(
        &broken,
        &AuditOptions {
            min_pt: Some(30.0),
            strict: false,
        },
    )
    .unwrap();
    assert!(floor
        .findings
        .iter()
        .any(|f| f.kind == FindingKind::BelowMinPt && f.shape == "Left"));
}

#[test]
fn a_picture_over_text_is_an_overlap() {
    let md = "## P\n\n![A photo](images/photo-8x6.jpg)\n";
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    let clean = convert(md, Format::Pptx, &o).unwrap().bytes;
    let broken = patch(&clean, "ppt/slides/slide1.xml", |xml| {
        // A text box right under the picture's middle.
        let start = xml.find("<p:pic>").unwrap();
        let off = &xml[start..];
        let x: i64 = off
            .split("<a:off x=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let y: i64 = off
            .split(" y=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let tb = text_box(
            50,
            "Hidden",
            (x + 100_000, y + 100_000, 2_000_000, 600_000),
            2400,
            "covered",
        );
        xml.replace("<p:pic>", &format!("{tb}<p:pic>"))
    });
    let report = audit(&broken, &AuditOptions::default()).unwrap();
    let hit = report
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::ImageOverText)
        .expect("picture over text");
    assert_eq!(hit.kind.as_str(), "overlap");
    assert!(hit.detail.contains("covers text 'Hidden'"));
}

/// The decks Pandoc made for md2star's own tests: the same findings `md2star audit
/// --min-pt 30 --json` reports (md2star 3.12), slide by slide and shape by shape.
#[test]
fn pandoc_decks_get_md2stars_findings() {
    type Expected<'a> = (&'a str, &'a [(usize, &'a str, &'a str)]);
    let expected: [Expected; 2] = [
        (
            "with_bib.pptx",
            &[
                (1, "ghost-placeholder", "Subtitle 2"),
                (2, "under-min-pt", "Content Placeholder 2"),
                (3, "under-min-pt", "Content Placeholder 2"),
            ],
        ),
        (
            "footnotes_document.pptx",
            &[
                (1, "ghost-placeholder", "Subtitle 2"),
                (2, "under-min-pt", "Content Placeholder 2"),
                (3, "under-min-pt", "Content Placeholder 2"),
                (4, "under-min-pt", "Content Placeholder 2"),
                (5, "under-min-pt", "Content Placeholder 2"),
                (6, "under-min-pt", "Content Placeholder 2"),
                (7, "under-min-pt", "Content Placeholder 2"),
                (8, "under-min-pt", "Content Placeholder 2"),
            ],
        ),
    ];
    for (file, want) in expected {
        let bytes = std::fs::read(fixtures().join("python-reference").join(file)).unwrap();
        let report = audit(
            &bytes,
            &AuditOptions {
                min_pt: Some(30.0),
                strict: false,
            },
        )
        .unwrap();
        let got: Vec<(usize, &str, &str)> = report
            .findings
            .iter()
            .map(|f| (f.slide, f.kind.as_str(), f.shape.as_str()))
            .collect();
        assert_eq!(got, want.to_vec(), "{file}");
    }
}

#[cfg(feature = "cli")]
#[test]
fn the_cli_speaks_md2stars_json_and_exit_codes() {
    use std::process::Command;
    let bin = env!("CARGO_BIN_EXE_md2star-rs");
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.pptx");
    std::fs::copy(fixtures().join("python-reference/with_bib.pptx"), &deck).unwrap();
    let out = Command::new(bin)
        .args(["audit", deck.to_str().unwrap(), "--json", "--min-pt", "30"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(json.trim_start().starts_with('['));
    assert!(json.contains("\"kind\": \"under-min-pt\""));
    assert!(json.contains("\"shape\": \"Content Placeholder 2\""));
    // A clean deck exits 0 with md2star's summary line.
    let clean = dir.path().join("clean.pptx");
    std::fs::write(
        &clean,
        convert("# Hi\n\n- a\n", Format::Pptx, &Options::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    let out = Command::new(bin)
        .args(["audit", clean.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "layout audit: clean (1 slides)"
    );
    // Unreadable: 2.
    let out = Command::new(bin)
        .args(["audit", dir.path().join("missing.pptx").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    std::fs::write(dir.path().join("junk.pptx"), b"junk").unwrap();
    let out = Command::new(bin)
        .args(["audit", dir.path().join("junk.pptx").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

// ── low-contrast (md2star's `TestContrast`, plus theme colours) ─────────────────

fn panel(id: u32, frame: (i64, i64, i64, i64), fill: &str) -> String {
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Panel {id}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill>{fill}</a:solidFill></p:spPr></p:sp>",
        frame.0, frame.1, frame.2, frame.3
    )
}

fn coloured_box(sz: u32, colour: &str) -> String {
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"61\" name=\"Readable\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"914400\" y=\"914400\"/><a:ext cx=\"4572000\" cy=\"1371600\"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"{sz}\"><a:solidFill>{colour}</a:solidFill></a:rPr><a:t>Readable?</a:t></a:r></a:p></p:txBody></p:sp>"
    )
}

fn srgb(hex: &str) -> String {
    format!("<a:srgbClr val=\"{hex}\"/>")
}

/// A one-slide deck, a panel (on the slide, or on its layout) behind a coloured run.
fn contrast_deck(text: &str, panel_fill: Option<&str>, pt: u32, on_layout: bool) -> Vec<u8> {
    let clean = convert("# One\n\n- a\n", Format::Pptx, &Options::default())
        .unwrap()
        .bytes;
    let frame = (0, 0, 8_229_600, 5_486_400);
    let rels = pptx_common::Deck::open(&clean)
        .part("ppt/slides/_rels/slide1.xml.rels")
        .unwrap();
    let layout = rels
        .split("slideLayouts/")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .map(|f| format!("ppt/slideLayouts/{f}"))
        .unwrap();
    let mut deck = clean;
    if let (Some(fill), true) = (panel_fill, on_layout) {
        let p = panel(60, frame, fill);
        deck = patch(&deck, &layout, |xml| {
            xml.replacen("</p:spTree>", &format!("{p}</p:spTree>"), 1)
        });
    }
    let under = match (panel_fill, on_layout) {
        (Some(fill), false) => panel(60, frame, fill),
        _ => String::new(),
    };
    let tb = coloured_box(pt * 100, text);
    patch(&deck, "ppt/slides/slide1.xml", |xml| {
        xml.replacen("</p:spTree>", &format!("{under}{tb}</p:spTree>"), 1)
    })
}

fn contrast_findings(deck: &[u8]) -> Vec<md2star_rs::audit::Finding> {
    audit(deck, &AuditOptions::default())
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.kind == FindingKind::LowContrast)
        .collect()
}

#[test]
fn pale_grey_on_near_white_is_flagged() {
    let found = contrast_findings(&contrast_deck(
        &srgb("C8C8C8"),
        Some(&srgb("F2F2F2")),
        28,
        false,
    ));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind.as_str(), "low-contrast");
    assert_eq!(found[0].shape, "Readable");
    assert_eq!(
        found[0].detail,
        "#C8C8C8 on #F2F2F2 is 1.5:1, under the 3:1 floor"
    );
    assert!(audit(
        &contrast_deck(&srgb("C8C8C8"), Some(&srgb("F2F2F2")), 28, false),
        &AuditOptions::default()
    )
    .unwrap()
    .fails(false));
}

#[test]
fn black_on_white_is_not() {
    let found = contrast_findings(&contrast_deck(
        &srgb("000000"),
        Some(&srgb("FFFFFF")),
        28,
        false,
    ));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn the_panel_a_heading_sits_on_counts_even_from_the_layout() {
    let dark = srgb("141416");
    assert_eq!(
        contrast_findings(&contrast_deck(&srgb("000000"), Some(&dark), 28, true)).len(),
        1
    );
    assert!(contrast_findings(&contrast_deck(&srgb("FFFFFF"), Some(&dark), 28, true)).is_empty());
}

#[test]
fn small_type_is_held_to_the_stricter_floor() {
    // 3.0:1 passes the large-type floor and fails the body-copy one.
    let (pale, white) = (srgb("949494"), srgb("FFFFFF"));
    assert!(contrast_findings(&contrast_deck(&pale, Some(&white), 28, false)).is_empty());
    let small = contrast_findings(&contrast_deck(&pale, Some(&white), 12, false));
    assert_eq!(small.len(), 1);
    assert!(
        small[0].detail.ends_with("under the 4.5:1 floor"),
        "{}",
        small[0].detail
    );
}

#[test]
fn a_backdrop_nobody_can_read_is_left_unjudged() {
    // A gradient panel behind white text: no single colour, so no finding.
    let gradient = "<p:sp><p:nvSpPr><p:cNvPr id=\"60\" name=\"Gradient\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"8229600\" cy=\"5486400\"/></a:xfrm><a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs><a:gs pos=\"100000\"><a:srgbClr val=\"FAFAFA\"/></a:gs></a:gsLst></a:gradFill></p:spPr></p:sp>";
    let clean = contrast_deck(&srgb("FAFAFA"), None, 28, false);
    let deck = patch(&clean, "ppt/slides/slide1.xml", |xml| {
        xml.replacen(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"61\"",
            &format!("{gradient}<p:sp><p:nvSpPr><p:cNvPr id=\"61\""),
            1,
        )
    });
    assert!(contrast_findings(&deck).is_empty());
}

#[test]
fn theme_colours_are_resolved_through_the_colour_map() {
    // Beyond md2star (python-pptx reads RGB only): `bg1` text on a `bg1` panel is the
    // same colour twice, whatever the theme says it is.
    let bg1 = "<a:schemeClr val=\"bg1\"/>";
    let found = contrast_findings(&contrast_deck(bg1, Some(bg1), 28, false));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].detail.contains(" is 1.0:1"), "{}", found[0].detail);
    // `tx1` on `bg1` is the theme's own text on its own background: readable.
    let tx1 = "<a:schemeClr val=\"tx1\"/>";
    assert!(contrast_findings(&contrast_deck(tx1, Some(bg1), 28, false)).is_empty());
}
