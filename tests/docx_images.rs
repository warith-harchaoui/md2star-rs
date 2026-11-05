//! Images in DOCX: embedded parts, relationships, sizes, alt text, figures and captions,
//! SVG with its raster fallback, and the warnings for what cannot be embedded.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use md2star_rs::resources::EMU_PER_INCH;
use md2star_rs::{convert, Format, Options, Rendered};

fn images() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images")
}

fn docx(md: &str) -> Rendered {
    let mut options = Options::default();
    options.base_dir = Some(images());
    convert(md, Format::Docx, &options).expect("converts")
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
    String::from_utf8(parts[name].clone()).unwrap()
}

fn media(parts: &BTreeMap<String, Vec<u8>>) -> Vec<&String> {
    parts
        .keys()
        .filter(|k| k.starts_with("word/media/"))
        .collect()
}

/// md2star's bundled template (the default): A4, half-inch margins.
const TEXT_WIDTH_EMU: u64 = (11900 - 2 * 720) * 635;

#[test]
fn a_png_is_embedded_at_its_size_with_its_alt_text() {
    let out = docx("Inline ![tiny png](pixel-4x3.png){width=1in} here.\n");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let p = parts(&out.bytes);
    let xml = text(&p, "word/document.xml");
    assert_eq!(media(&p), vec!["word/media/image1.png"]);
    assert_eq!(
        p["word/media/image1.png"],
        std::fs::read(images().join("pixel-4x3.png")).unwrap()
    );
    // 1 in wide, the aspect ratio (4×3) keeps the height at 0.75 in.
    assert!(xml.contains(&format!(
        "<wp:extent cx=\"{}\" cy=\"{}\"/>",
        EMU_PER_INCH,
        EMU_PER_INCH * 3 / 4
    )));
    assert!(xml.contains("descr=\"tiny png\""));
    assert!(xml.contains("<wp:inline "), "inline, in the paragraph");
    let rels = text(&p, "word/_rels/document.xml.rels");
    assert!(rels.contains("relationships/image\" Target=\"media/image1.png\""));
    let ct = text(&p, "[Content_Types].xml");
    assert!(ct.contains("<Default Extension=\"png\" ContentType=\"image/png\"/>"));
}

#[test]
fn a_figure_is_centred_with_its_caption_and_bookmark() {
    let out = docx("![A *captioned* figure](photo-8x6.jpg){#fig:photo width=50%}\n");
    let p = parts(&out.bytes);
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains("<w:pStyle w:val=\"CaptionedFigure\"/><w:jc w:val=\"center\"/>"));
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"fig:photo\"/>"));
    let caption = xml
        .find("<w:pStyle w:val=\"ImageCaption\"/>")
        .expect("a caption");
    assert!(
        caption > xml.find("<w:drawing>").unwrap(),
        "caption below the picture"
    );
    assert!(xml[caption..]
        .contains("<w:i/><w:iCs/></w:rPr><w:t xml:space=\"preserve\">captioned</w:t>"));
    // 50% of the text column, aspect 8:6.
    let cx = TEXT_WIDTH_EMU / 2;
    assert!(
        xml.contains(&format!(
            "<wp:extent cx=\"{cx}\" cy=\"{}\"/>",
            (cx as f64 * 6.0 / 8.0).round() as u64
        )),
        "{xml}"
    );
    assert!(xml.contains("descr=\"A captioned figure\""));
    assert_eq!(media(&p), vec!["word/media/image1.jpeg"]);
}

#[test]
fn an_image_alone_without_alt_text_is_no_captioned_figure() {
    // Pandoc's rule: an implicit figure needs a caption; without one the image stays
    // an inline picture in its paragraph.
    let xml = text(
        &parts(&docx("![](pixel-4x3.png)\n").bytes),
        "word/document.xml",
    );
    assert!(xml.contains("<w:drawing>"));
    assert!(!xml.contains("ImageCaption"));
    assert!(!xml.contains("CaptionedFigure"));
}

#[test]
fn an_svg_carries_a_raster_fallback() {
    let out = docx("![Vector diagram](diagram.svg)\n");
    let p = parts(&out.bytes);
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains("<asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed="));
    assert_eq!(
        media(&p),
        vec!["word/media/image1.png", "word/media/image2.svg"]
    );
    assert_eq!(
        p["word/media/image2.svg"],
        std::fs::read(images().join("diagram.svg")).unwrap()
    );
    // 200×100 CSS pixels.
    assert!(xml.contains("<wp:extent cx=\"1905000\" cy=\"952500\"/>"));
    let png = md2star_rs::resources::probe(&p["word/media/image1.png"]).unwrap();
    if cfg!(feature = "svg") {
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(
            (png.width_px, png.height_px),
            (400, 200),
            "rasterised at 2x"
        );
    } else {
        assert!(out
            .warnings
            .iter()
            .any(|w| w.code == "svg-without-fallback"));
    }
}

#[test]
fn a_data_uri_image_is_embedded() {
    let out = docx("A dot ![dot](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC).\n");
    assert!(out.warnings.is_empty());
    assert_eq!(media(&parts(&out.bytes)).len(), 1);
}

#[test]
fn the_same_image_twice_is_stored_once() {
    let out = docx("![a](pixel-4x3.png) and ![b](pixel-4x3.png)\n");
    let p = parts(&out.bytes);
    assert_eq!(media(&p).len(), 1);
    let xml = text(&p, "word/document.xml");
    assert_eq!(xml.matches("<w:drawing>").count(), 2);
    let rels = text(&p, "word/_rels/document.xml.rels");
    assert_eq!(rels.matches("relationships/image\"").count(), 1);
    assert!(
        xml.contains("<wp:docPr id=\"1\"") && xml.contains("<wp:docPr id=\"2\""),
        "drawing ids are unique"
    );
}

#[test]
fn a_large_image_is_scaled_down_to_the_column() {
    let out = docx("![wide](pixel-4x3.png){width=40in}\n");
    let xml = text(&parts(&out.bytes), "word/document.xml");
    assert!(
        xml.contains(&format!(
            "<wp:extent cx=\"{TEXT_WIDTH_EMU}\" cy=\"{}\"/>",
            (TEXT_WIDTH_EMU as f64 * 0.75).round() as u64
        )),
        "{xml}"
    );
}

#[test]
fn an_image_in_a_table_fits_its_column() {
    let out = docx("| a | b |\n|---|---|\n| ![x](pixel-4x3.png){width=40in} | y |\n");
    let xml = text(&parts(&out.bytes), "word/document.xml");
    let col = (11900 - 2 * 720) / 2 * 635;
    assert!(xml.contains(&format!("<wp:extent cx=\"{col}\"")), "{xml}");
}

#[test]
fn what_cannot_be_embedded_is_a_warning_and_its_alt_text() {
    let out = docx("![missing](nope.png) ![remote](https://example.invalid/x.png)\n");
    let codes: Vec<&str> = out.warnings.iter().map(|w| w.code).collect();
    assert_eq!(codes, vec!["image-not-embedded", "image-not-embedded"]);
    assert!(out.warnings[1].message.contains("--allow-remote-images"));
    let p = parts(&out.bytes);
    assert!(media(&p).is_empty());
    let xml = text(&p, "word/document.xml");
    assert!(xml.contains(">[missing]</w:t>") && xml.contains(">[remote]</w:t>"));
    // WebP: Word cannot show it.
    let webp = "data:image/webp;base64,UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA";
    let out = docx(&format!("![w]({webp})\n"));
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "image-not-embedded" && w.message.contains("WebP")));
}

#[test]
fn a_mermaid_diagram_is_a_picture_with_the_feature_and_source_without() {
    let out = docx("```mermaid\nflowchart LR\n  A --> B\n```\n");
    let p = parts(&out.bytes);
    let xml = text(&p, "word/document.xml");
    let warned = out
        .warnings
        .iter()
        .any(|w| w.code == "mermaid-not-rendered");
    if cfg!(feature = "mermaid") {
        assert!(!warned, "{:?}", out.warnings);
        assert!(xml.contains("<w:pStyle w:val=\"Figure\"/>"), "{xml}");
        assert!(xml.contains("descr=\"Mermaid diagram\""), "{xml}");
        assert!(!xml.contains("A --&gt; B"), "the source is gone");
        // The SVG and, with `svg`, the PNG Word shows in its place.
        assert!(
            media(&p).iter().any(|m| m.ends_with(".svg")),
            "{:?}",
            media(&p)
        );
        assert!(!xml.contains("<w:pStyle w:val=\"SourceCode\"/>"));
    } else {
        assert!(warned);
        assert!(xml.contains("A --&gt; B"), "the source stays readable");
    }
}
