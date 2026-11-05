//! Pictures on slides.
//!
//! An [`crate::ast::Image`] is loaded through [`crate::resources::Resolver`] (once per
//! deck), placed by [`super::fit`] into the layout's picture placeholder or a body box,
//! **contain-fitted** there (aspect kept, centred: a picture is never cropped or
//! stretched), sized down further by `{width=…}` / `{height=…}` when the author asks
//! ([`fit_in`]), and written as a `<p:pic>` ([`pic_xml`]) whose description is the alt
//! text. SVG goes in as `asvg:svgBlip` with a PNG fallback, the way PowerPoint itself
//! stores it: rasterised in process by `resvg` with the `svg` feature (on by default,
//! the same one the DOCX writer uses), a transparent pixel without it. An image that cannot be loaded is a warning and its alt
//! text in italics, not a failed deck. A picture never covers text: the fit model gives
//! each its own box or stacks them (the audit checks it).

use crate::ast::Attr;
use crate::resources::{fit_emu, ImageInfo};

use super::xml::esc;

/// Size `info` inside `frame` (already a contain-fit box or a placeholder box), honouring
/// `width=` / `height=` relative to the frame, centred in it.
///
/// # Examples
///
/// ```
/// use md2star_rs::ast::Attr;
/// use md2star_rs::pptx::images::fit_in;
/// use md2star_rs::resources::{ImageFormat, ImageInfo};
///
/// let info = ImageInfo { format: ImageFormat::Png, width_px: 400, height_px: 200 };
/// let half = Attr { kv: vec![("width".into(), "50%".into())], ..Attr::default() };
/// assert_eq!(fit_in(&info, &half, (0, 0, 1000, 500)), (250, 125, 500, 250));
/// // No attribute: the frame itself.
/// assert_eq!(fit_in(&info, &Attr::default(), (0, 0, 1000, 500)), (0, 0, 1000, 500));
/// ```
pub fn fit_in(info: &ImageInfo, attr: &Attr, frame: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    if attr.get("width").is_none() && attr.get("height").is_none() {
        return frame;
    }
    let (x, y, w, h) = frame;
    let (fw, fh) = fit_emu(info, attr, w.max(1) as u64, h.max(1) as u64);
    let (fw, fh) = (fw as i64, fh as i64);
    (x + (w - fw) / 2, y + (h - fh) / 2, fw, fh)
}

/// The `<p:pic>` element: shape `shape_id`, `descr` as its accessible description,
/// `blip` its `<a:blip>`, `hlink` an optional `<a:hlinkClick>` on the picture itself.
pub fn pic_xml(
    shape_id: u32,
    descr: &str,
    blip: &str,
    hlink: &str,
    frame: (i64, i64, i64, i64),
) -> String {
    format!(
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{shape_id}\" name=\"Picture {}\" descr=\"{}\">{hlink}</p:cNvPr><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill>{blip}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        shape_id.saturating_sub(1),
        esc(descr),
        frame.0,
        frame.1,
        frame.2.max(1),
        frame.3.max(1)
    )
}

/// The `<a:blip>` of a raster picture.
pub fn raster_blip(rel_id: &str) -> String {
    format!("<a:blip r:embed=\"{rel_id}\"/>")
}

/// The `<a:blip>` of an SVG: the PNG stand-in, and the SVG in PowerPoint's extension.
pub fn svg_blip(png_rel: &str, svg_rel: &str) -> String {
    format!(
        "<a:blip r:embed=\"{png_rel}\"><a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\"><asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"{svg_rel}\"/></a:ext></a:extLst></a:blip>"
    )
}

/// The PNG fallback of an SVG drawn `width_px` × `height_px` CSS pixels: rendered at
/// twice that size (crisp on dense screens), the longer side capped at 4096 pixels.
/// `None` when the SVG cannot be read or the `svg` feature is off.
#[cfg(feature = "svg")]
pub fn svg_fallback_png(svg: &[u8], width_px: u32, height_px: u32) -> Option<Vec<u8>> {
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
    let cap = (4096.0 / w.max(h)).min(1.0);
    let (w, h) = ((w * cap).round().max(1.0), (h * cap).round().max(1.0));
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    let transform =
        tiny_skia::Transform::from_scale(w / size.width().max(1.0), h / size.height().max(1.0));
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// Without the `svg` feature there is no rasteriser: the caller keeps its stand-in.
#[cfg(not(feature = "svg"))]
pub fn svg_fallback_png(svg: &[u8], width_px: u32, height_px: u32) -> Option<Vec<u8>> {
    let _ = (svg, width_px, height_px);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_carries_its_description_escaped() {
        let x = pic_xml(4, "a < b", &raster_blip("rId2"), "", (1, 2, 3, 4));
        assert!(x.contains("descr=\"a &lt; b\""));
        assert!(x.contains("r:embed=\"rId2\""));
        assert!(x.contains("<a:off x=\"1\" y=\"2\"/><a:ext cx=\"3\" cy=\"4\"/>"));
    }

    #[cfg(feature = "svg")]
    #[test]
    fn an_svg_is_rasterised_for_its_fallback() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#2a6"/></svg>"##;
        let png = svg_fallback_png(svg, 40, 20).unwrap();
        let info = crate::resources::probe(&png).unwrap();
        assert_eq!((info.width_px, info.height_px), (80, 40));
        assert!(svg_fallback_png(b"not svg", 1, 1).is_none());
    }

    #[test]
    fn an_svg_keeps_a_png_stand_in() {
        let b = svg_blip("rId2", "rId3");
        assert!(b.starts_with("<a:blip r:embed=\"rId2\">"));
        assert!(b.contains("svgBlip"));
    }
}
