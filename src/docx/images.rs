//! Images embedded in a Word document.
//!
//! The bytes come from [`crate::resources::Resolver`]; the size from
//! [`crate::resources::fit_emu`] (the image's pixels at 96 dpi, the author's `width=` /
//! `height=`, then scaled down to the text column, never up). Each distinct file is
//! stored once under `word/media/`, whatever the number of places that show it.
//!
//! What Word can show, and what we do:
//!
//! - PNG, JPEG, GIF, BMP, TIFF: embedded as they are.
//! - SVG: Word 2016 and later draw an SVG from an `asvg:svgBlip` extension, but the
//!   picture's main `a:blip` must still be a raster image — the fallback every other
//!   reader (older Word, LibreOffice, previews) shows. With the `svg` feature (on by
//!   default) the SVG is rasterised in process by `resvg` into that PNG fallback, as
//!   Pandoc does with `rsvg-convert`. Without it, the SVG is embedded with a transparent
//!   placeholder as the fallback, and a warning says that readers without SVG support
//!   show an empty frame.
//! - WebP, ICO: Word cannot show them; the image is not embedded, a warning says so and
//!   its alt text stands in its place.
//!
//! Owned by the DOCX port (see [`crate::docx`]).

use std::collections::HashMap;

use super::xml::escape_attr;
use crate::resources::{ImageFormat, LoadedImage};

/// The image files of the package, deduplicated by content.
#[derive(Debug, Default)]
pub(super) struct Media {
    files: Vec<(String, Vec<u8>)>,
    by_content: HashMap<Vec<u8>, String>,
}

impl Media {
    /// Store `bytes` as `word/media/imageN.ext` (once per distinct content); returns
    /// the target relative to `word/` (`media/image1.png`).
    pub(super) fn store(&mut self, bytes: Vec<u8>, ext: &str) -> String {
        if let Some(target) = self.by_content.get(&bytes) {
            return target.clone();
        }
        let target = format!("media/image{}.{ext}", self.files.len() + 1);
        self.by_content.insert(bytes.clone(), target.clone());
        self.files.push((format!("word/{target}"), bytes));
        target
    }

    /// The parts to pack.
    pub(super) fn into_parts(self) -> Vec<(String, Vec<u8>)> {
        self.files
    }
}

/// An image ready to embed: the raster Word draws, and the SVG original when there is
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Prepared {
    /// The raster image (`a:blip`).
    pub raster: Vec<u8>,
    /// Its extension (`png`, `jpeg`…).
    pub ext: &'static str,
    /// The SVG, for `asvg:svgBlip`.
    pub svg: Option<Vec<u8>>,
    /// Set when the raster is only a placeholder.
    pub placeholder: bool,
}

/// Turn a loaded image into what Word embeds.
///
/// # Errors
///
/// A message when Word cannot show the format (WebP, ICO).
pub(super) fn prepare(image: &LoadedImage) -> std::result::Result<Prepared, String> {
    let raster = |ext: &'static str| Prepared {
        raster: image.bytes.clone(),
        ext,
        svg: None,
        placeholder: false,
    };
    match image.info.format {
        ImageFormat::Png => Ok(raster("png")),
        ImageFormat::Jpeg => Ok(raster("jpeg")),
        ImageFormat::Gif => Ok(raster("gif")),
        ImageFormat::Bmp => Ok(raster("bmp")),
        ImageFormat::Tiff => Ok(raster("tiff")),
        ImageFormat::Svg => Ok(
            match rasterise_svg(&image.bytes, image.info.width_px, image.info.height_px) {
                Some(png) => Prepared {
                    raster: png,
                    ext: "png",
                    svg: Some(image.bytes.clone()),
                    placeholder: false,
                },
                None => Prepared {
                    raster: TRANSPARENT_PNG.to_vec(),
                    ext: "png",
                    svg: Some(image.bytes.clone()),
                    placeholder: true,
                },
            },
        ),
        ImageFormat::Webp => Err("Word cannot show WebP images: convert it to PNG or JPEG".into()),
        ImageFormat::Ico => Err("Word cannot show ICO images: convert it to PNG".into()),
    }
}

/// A 1×1 transparent PNG: the fallback of an SVG we could not rasterise.
const TRANSPARENT_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Rasterise an SVG to PNG at twice its CSS pixel size (crisp on high-density screens),
/// the longer side capped at 4096 pixels. `None` when the SVG cannot be read.
#[cfg(feature = "svg")]
fn rasterise_svg(svg: &[u8], width_px: u32, height_px: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    use std::sync::{Arc, OnceLock};

    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });
    let options = usvg::Options {
        fontdb: Arc::clone(fonts),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(svg, &options).ok()?;
    let (w, h) = (width_px.max(1) as f32 * 2.0, height_px.max(1) as f32 * 2.0);
    let scale_cap = (4096.0 / w.max(h)).min(1.0);
    let (w, h) = (
        (w * scale_cap).round().max(1.0),
        (h * scale_cap).round().max(1.0),
    );
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    let transform =
        tiny_skia::Transform::from_scale(w / size.width().max(1.0), h / size.height().max(1.0));
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

#[cfg(not(feature = "svg"))]
fn rasterise_svg(_svg: &[u8], _width_px: u32, _height_px: u32) -> Option<Vec<u8>> {
    None
}

/// What a drawing shows and says.
pub(super) struct Drawing<'a> {
    /// The raster's relationship id.
    pub rid: &'a str,
    /// The SVG's relationship id, if any.
    pub svg_rid: Option<&'a str>,
    /// Size in EMU.
    pub cx: u64,
    pub cy: u64,
    /// The document-wide unique drawing id.
    pub id: usize,
    /// Alt text (`descr`).
    pub descr: &'a str,
    /// Title (`![a](x.png "title")`).
    pub title: &'a str,
    /// The source as written (the picture's name, as Pandoc sets it).
    pub name: &'a str,
}

/// The `<w:r><w:drawing>…</w:drawing></w:r>` run that shows an inline picture.
///
/// The namespaces (`wp`, `a`, `pic`, `r`) are declared on the document root.
pub(super) fn drawing_run(d: &Drawing<'_>) -> String {
    let ext = match d.svg_rid {
        Some(svg) => format!(
            "<a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\">\
             <asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"{svg}\"/>\
             </a:ext></a:extLst>"
        ),
        None => String::new(),
    };
    let blip = if ext.is_empty() {
        format!("<a:blip r:embed=\"{}\"/>", d.rid)
    } else {
        format!("<a:blip r:embed=\"{}\">{ext}</a:blip>", d.rid)
    };
    let title = if d.title.is_empty() {
        String::new()
    } else {
        format!(" title=\"{}\"", escape_attr(d.title))
    };
    format!(
        "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
         <wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"{descr}\"{title}/>\
         <wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"{name}\" descr=\"{descr}\"/>\
         <pic:cNvPicPr><a:picLocks noChangeAspect=\"1\" noChangeArrowheads=\"1\"/></pic:cNvPicPr></pic:nvPicPr>\
         <pic:blipFill>{blip}<a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr bwMode=\"auto\"><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></pic:spPr></pic:pic>\
         </a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
        cx = d.cx,
        cy = d.cy,
        id = d.id,
        descr = escape_attr(d.descr),
        name = escape_attr(d.name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{ImageInfo, Origin};

    fn loaded(format: ImageFormat, bytes: &[u8]) -> LoadedImage {
        LoadedImage {
            bytes: bytes.to_vec(),
            info: ImageInfo {
                format,
                width_px: 20,
                height_px: 10,
            },
            origin: Origin::DataUri,
        }
    }

    #[test]
    fn media_is_stored_once_per_content() {
        let mut media = Media::default();
        assert_eq!(media.store(vec![1, 2], "png"), "media/image1.png");
        assert_eq!(media.store(vec![3], "jpeg"), "media/image2.jpeg");
        assert_eq!(media.store(vec![1, 2], "png"), "media/image1.png");
        assert_eq!(media.into_parts().len(), 2);
    }

    #[test]
    fn word_formats_are_embedded_as_they_are() {
        let p = prepare(&loaded(ImageFormat::Jpeg, b"jpeg bytes")).unwrap();
        assert_eq!(
            (p.ext, p.raster.as_slice(), p.svg),
            ("jpeg", &b"jpeg bytes"[..], None)
        );
        assert!(prepare(&loaded(ImageFormat::Webp, b"x")).is_err());
        assert!(prepare(&loaded(ImageFormat::Ico, b"x")).is_err());
    }

    #[test]
    fn an_svg_carries_a_png_fallback() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#c00"/></svg>"##;
        let p = prepare(&loaded(ImageFormat::Svg, svg)).unwrap();
        assert_eq!(p.ext, "png");
        assert_eq!(p.svg.as_deref(), Some(&svg[..]));
        assert_eq!(&p.raster[..8], b"\x89PNG\r\n\x1a\n");
        if cfg!(feature = "svg") {
            assert!(!p.placeholder);
            let info = crate::resources::probe(&p.raster).unwrap();
            assert_eq!((info.width_px, info.height_px), (40, 20), "rendered at 2x");
        } else {
            assert!(p.placeholder);
        }
    }

    #[test]
    fn the_drawing_names_its_parts_and_alt_text() {
        let xml = drawing_run(&Drawing {
            rid: "rId9",
            svg_rid: Some("rId10"),
            cx: 914_400,
            cy: 457_200,
            id: 3,
            descr: "A \"quoted\" alt",
            title: "",
            name: "img.svg",
        });
        assert!(xml.contains("<wp:extent cx=\"914400\" cy=\"457200\"/>"));
        assert!(xml.contains(
            "<wp:docPr id=\"3\" name=\"Picture 3\" descr=\"A &quot;quoted&quot; alt\"/>"
        ));
        assert!(xml.contains("<a:blip r:embed=\"rId9\"><a:extLst>"));
        assert!(xml.contains("r:embed=\"rId10\"/>"));
    }
}
