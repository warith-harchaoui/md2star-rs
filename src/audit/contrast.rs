//! `low-contrast`: text a reader cannot separate from what is behind it.
//!
//! The port of md2star's check (`pptx_audit._check_contrast`): the WCAG 2.x contrast
//! ratio (sRGB-linearised relative luminance) of each run that states its own colour,
//! against the backdrop a renderer would paint behind its shape, held to 3:1 for large
//! type (18 pt, or 14 pt bold) and 4.5:1 for the rest.
//!
//! The backdrop is resolved in md2star's order: the shape's own fill (a code panel),
//! then the shapes under it on the slide, the slide's background, the layout's shapes
//! and background, the master's shapes and background. A shape counts as a backdrop
//! only when it contains the text box whole: a box straddling two backgrounds has no
//! single ratio. A picture, a gradient or a pattern stops the search, unjudged.
//!
//! Where this goes further than md2star (python-pptx reads `srgbClr` only): theme
//! colours (`schemeClr`, through the master's `clrMap` and any `clrMapOvr`) and system
//! colours are resolved, with their `lumMod` / `lumOff` / `shade` / `tint`
//! modifiers, and so are fills and backgrounds given by reference to the theme's fill
//! styles (`fillRef`, `bgRef`) when the referenced style is a solid fill. A run that
//! inherits its colour from the template is still left alone, as md2star leaves it:
//! the designer chose it.

use std::collections::HashMap;

use crate::pptx::template::{shapes, xfrm_of, Package};
use crate::pptx::xml::Node;

use super::Box4;

/// WCAG's "large" type: 18 pt, or 14 pt bold.
pub const CONTRAST_LARGE_PT: f32 = 18.0;
/// WCAG's "large" type when bold.
pub const CONTRAST_LARGE_BOLD_PT: f32 = 14.0;
/// The floor for large type.
pub const CONTRAST_MIN_LARGE: f64 = 3.0;
/// The floor for everything else.
pub const CONTRAST_MIN_NORMAL: f64 = 4.5;

/// An sRGB colour.
pub(super) type Rgb = [u8; 3];

/// What a shape or a background paints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Fill {
    /// Nothing: look further back.
    None,
    /// One opaque colour.
    Solid(Rgb),
    /// Something no single colour describes (a picture, a gradient, a pattern, a
    /// translucent or unresolvable colour): stop looking, judge nothing.
    Opaque,
}

/// A part's colour context: the theme's colours seen through a colour map, and its
/// fill styles.
#[derive(Debug, Clone, Default)]
pub(super) struct Palette {
    /// `dk1`, `lt1`, `accent1`… → colour.
    scheme: HashMap<String, Rgb>,
    /// `bg1` → `lt1`, `tx1` → `dk1`…
    map: HashMap<String, String>,
    /// `a:fillStyleLst` children (referenced by `fillRef idx` 1…).
    fill_styles: Vec<Node>,
    /// `a:bgFillStyleLst` children (referenced by `idx` 1001…).
    bg_fill_styles: Vec<Node>,
}

impl Palette {
    /// The palette of a master: its theme and its `clrMap`.
    pub(super) fn for_master(package: &Package, master_part: &str, master: &Node) -> Self {
        let mut palette = Palette::default();
        if let Some(theme) = package
            .rel_of(master_part, "theme")
            .and_then(|p| package.xml(&p).ok())
        {
            if let Some(scheme) = theme.descendants("clrScheme").first() {
                for slot in &scheme.children {
                    if let Some(rgb) = slot.children.first().and_then(base_colour) {
                        palette.scheme.insert(slot.local().to_string(), rgb);
                    }
                }
            }
            if let Some(list) = theme.descendants("fillStyleLst").first() {
                palette.fill_styles = list.children.clone();
            }
            if let Some(list) = theme.descendants("bgFillStyleLst").first() {
                palette.bg_fill_styles = list.children.clone();
            }
        }
        if let Some(map) = master.child("clrMap") {
            palette.set_map(map);
        }
        palette
    }

    fn set_map(&mut self, map: &Node) {
        for (k, v) in &map.attrs {
            self.map
                .insert(crate::pptx::xml::local(k).to_string(), v.clone());
        }
    }

    /// This palette with a slide's or a layout's `clrMapOvr` applied.
    pub(super) fn overridden(&self, part: &Node) -> Palette {
        let mut p = self.clone();
        if let Some(map) = part.path(&["clrMapOvr", "overrideClrMapping"]) {
            p.set_map(map);
        }
        p
    }

    /// The colour a colour element (`a:srgbClr`, `a:schemeClr`, `a:sysClr`) names,
    /// modifiers applied; `None` when it cannot be resolved or is translucent.
    /// `ph` is what `phClr` stands for (a style reference's own colour).
    pub(super) fn colour(&self, el: &Node, ph: Option<Rgb>) -> Option<Rgb> {
        let base = match el.local() {
            "schemeClr" => {
                let name = el.attr("val")?;
                if name == "phClr" {
                    ph?
                } else {
                    let slot = self.map.get(name).map_or(name, String::as_str);
                    *self.scheme.get(slot)?
                }
            }
            _ => base_colour(el)?,
        };
        apply_modifiers(base, el)
    }

    /// The colour inside a fill's or a run's `a:solidFill`.
    fn solid(&self, solid: &Node, ph: Option<Rgb>) -> Fill {
        match solid.children.first().and_then(|c| self.colour(c, ph)) {
            Some(rgb) => Fill::Solid(rgb),
            None => Fill::Opaque,
        }
    }

    /// The fill a fill-properties element (`p:spPr`, `p:bgPr`) declares, `None` when it
    /// declares none.
    fn declared(&self, props: &Node) -> Option<Fill> {
        for c in &props.children {
            match c.local() {
                "noFill" => return Some(Fill::None),
                "solidFill" => return Some(self.solid(c, None)),
                "gradFill" | "pattFill" | "blipFill" | "grpFill" => return Some(Fill::Opaque),
                _ => {}
            }
        }
        None
    }

    /// A theme fill style, its `phClr` standing for `ph`.
    fn style(&self, style: Option<&Node>, ph: Option<Rgb>) -> Fill {
        match style.map(Node::local) {
            Some("noFill") => Fill::None,
            Some("solidFill") => self.solid(style.unwrap(), ph),
            Some(_) => Fill::Opaque,
            None => Fill::None,
        }
    }

    /// What a shape paints: its `spPr` fill, else its `p:style` `fillRef`; a picture
    /// is opaque.
    pub(super) fn shape_fill(&self, shape: &Node) -> Fill {
        if shape.local() == "pic" {
            return Fill::Opaque;
        }
        if !matches!(shape.local(), "sp" | "cxnSp") {
            return Fill::None;
        }
        if let Some(fill) = shape.child("spPr").and_then(|p| self.declared(p)) {
            return fill;
        }
        let Some(fill_ref) = shape.path(&["style", "fillRef"]) else {
            return Fill::None;
        };
        let idx = fill_ref.attr_i64("idx").unwrap_or(0);
        let ph = fill_ref.children.first().and_then(|c| self.colour(c, None));
        match idx {
            0 => Fill::None,
            1..=999 => self.style(self.fill_styles.get(idx as usize - 1), ph),
            _ => self.style(self.bg_fill_styles.get(idx as usize - 1001), ph),
        }
    }

    /// A slide's, layout's or master's own background (`p:bg`), `None` when it has none.
    pub(super) fn background(&self, part: &Node) -> Fill {
        let Some(bg) = part.path(&["cSld", "bg"]) else {
            return Fill::None;
        };
        if let Some(pr) = bg.child("bgPr") {
            return self.declared(pr).unwrap_or(Fill::None);
        }
        if let Some(r) = bg.child("bgRef") {
            let idx = r.attr_i64("idx").unwrap_or(0);
            let ph = r.children.first().and_then(|c| self.colour(c, None));
            return match idx {
                0 => Fill::None,
                1..=999 => self.style(self.fill_styles.get(idx as usize - 1), ph),
                _ => self.style(self.bg_fill_styles.get(idx as usize - 1001), ph),
            };
        }
        Fill::None
    }
}

/// `srgbClr` / `sysClr` without modifiers.
fn base_colour(el: &Node) -> Option<Rgb> {
    let hex = match el.local() {
        "srgbClr" => el.attr("val")?,
        "sysClr" => el.attr("lastClr")?,
        _ => return None,
    };
    parse_hex(hex)
}

fn parse_hex(hex: &str) -> Option<Rgb> {
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// DrawingML colour modifiers: `lumMod`/`lumOff` in HSL, `shade`/`tint` toward black
/// and white. Translucency (`alpha` under 100 %) has no single colour: `None`.
fn apply_modifiers(rgb: Rgb, el: &Node) -> Option<Rgb> {
    let pct = |c: &Node| c.attr_i64("val").map(|v| v as f64 / 100_000.0);
    let mut c = rgb.map(|v| v as f64 / 255.0);
    for m in &el.children {
        match m.local() {
            "alpha" if pct(m).is_some_and(|a| a < 0.999) => return None,
            "lumMod" | "lumOff" => {
                let v = pct(m)?;
                let (h, s, mut l) = to_hsl(c);
                if m.local() == "lumMod" {
                    l *= v;
                } else {
                    l += v;
                }
                c = from_hsl(h, s, l.clamp(0.0, 1.0));
            }
            "shade" => {
                let v = pct(m)?;
                c = c.map(|x| x * v);
            }
            "tint" => {
                let v = pct(m)?;
                c = c.map(|x| x + (1.0 - x) * (1.0 - v));
            }
            _ => {}
        }
    }
    Some(c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8))
}

fn to_hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

/// WCAG relative luminance, sRGB-linearised.
pub fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let c = v as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

/// The WCAG contrast ratio of two colours, 1:1 (identical) to 21:1 (black on white).
///
/// # Examples
///
/// ```
/// use md2star_rs::audit::contrast_ratio;
/// assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
/// assert!((contrast_ratio([255, 255, 255], [255, 255, 255]) - 1.0).abs() < 0.001);
/// ```
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (x, y) = (relative_luminance(a), relative_luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Whether `outer` contains `inner` whole.
pub(super) fn covers(outer: Box4, inner: Box4) -> bool {
    outer.0 <= inner.0
        && outer.1 <= inner.1
        && outer.0 + outer.2 >= inner.0 + inner.2
        && outer.1 + outer.3 >= inner.1 + inner.3
}

/// The shapes of a layout or a master as backdrops: (box, fill), in z-order.
pub(super) fn backdrops(part: &Node, palette: &Palette) -> Vec<(Box4, Fill)> {
    shapes(part)
        .into_iter()
        .filter_map(|s| Some((xfrm_of(s)?, palette.shape_fill(s))))
        .collect()
}

/// The topmost colour behind `bx` among `layers` (back to front): `Some(fill)` when
/// something here covers it, `None` when nothing does.
pub(super) fn backdrop_in(layers: &[(Box4, Fill)], bx: Box4) -> Option<Fill> {
    layers
        .iter()
        .rev()
        .filter(|(b, _)| covers(*b, bx))
        .map(|(_, f)| *f)
        .find(|f| *f != Fill::None)
}

/// One run worth judging: its own colour, size and weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ColouredRun {
    pub rgb: Rgb,
    pub size_pt: f32,
    pub bold: bool,
}

/// The worst run under its floor: (ratio, floor, run colour), if any.
pub(super) fn worst(runs: &[ColouredRun], behind: Rgb) -> Option<(f64, f64, Rgb)> {
    let mut worst: Option<(f64, f64, Rgb)> = None;
    for r in runs {
        let large =
            r.size_pt >= CONTRAST_LARGE_PT || (r.bold && r.size_pt >= CONTRAST_LARGE_BOLD_PT);
        let floor = if large {
            CONTRAST_MIN_LARGE
        } else {
            CONTRAST_MIN_NORMAL
        };
        let ratio = contrast_ratio(r.rgb, behind);
        if ratio < floor && worst.is_none_or(|w| ratio < w.0) {
            worst = Some((ratio, floor, r.rgb));
        }
    }
    worst
}

/// `C8C8C8`, as python-pptx prints an `RGBColor`.
pub(super) fn hex(rgb: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ratio_is_the_wcag_one() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
        // md2star's test pair: pale grey on near white is 1.5:1.
        let r = contrast_ratio([0xC8; 3], [0xF2; 3]);
        assert_eq!(format!("{r:.1}"), "1.5");
    }

    #[test]
    fn modifiers_follow_drawingml() {
        let node = |xml: &str| crate::pptx::xml::parse(xml.as_bytes()).unwrap();
        // Office's "white, darker 50 %": lumMod 50 % of lightness 1.0.
        let el = node(r#"<a:srgbClr val="FFFFFF"><a:lumMod val="50000"/></a:srgbClr>"#);
        assert_eq!(apply_modifiers([255; 3], &el), Some([128; 3]));
        // "black, lighter 25 %": lumMod 75 % + lumOff 25 % of lightness 0.
        let el = node(
            r#"<a:srgbClr val="000000"><a:lumMod val="75000"/><a:lumOff val="25000"/></a:srgbClr>"#,
        );
        assert_eq!(apply_modifiers([0; 3], &el), Some([64; 3]));
        let el = node(r#"<a:srgbClr val="000000"><a:alpha val="50000"/></a:srgbClr>"#);
        assert_eq!(
            apply_modifiers([0; 3], &el),
            None,
            "translucent: no single colour"
        );
    }
}
