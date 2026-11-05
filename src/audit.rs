//! `audit`: what is wrong with a deck, read off its OOXML.
//!
//! A port of `md2star audit` (`md2star/pptx_audit.py`), deterministic and portable: no
//! model, no render, no network — pure XML over the zip, so it runs on a phone. It works
//! on any `.pptx`, not only the ones md2star-rs built. For every slide, each shape's box
//! is resolved through its layout and master placeholders (and group transforms), and
//! each run's size through the run, the paragraph level, the shape's list style, the
//! layout and master placeholders, the master's text styles and the presentation's
//! defaults — the sizes `--min-pt` leaves to the template are checked like the others.
//!
//! The findings, in md2star's closed vocabulary and severity order:
//!
//! - **off-slide**: a shape reaching past the canvas by more than 1 % of it;
//! - **overlap**: two text shapes sharing more than 10 % of the smaller box, or a
//!   picture covering more than 10 % of a text box (a backdrop of 80 % of the canvas or
//!   more is exempt) — the latter is [`FindingKind::ImageOverText`], reported under the
//!   same `overlap` kind as md2star does;
//! - **ghost-placeholder**: a placeholder left empty (date, footer and slide number
//!   excepted: PowerPoint fills those);
//! - **illegible-type**: text under 12 pt;
//! - **under-min-pt**: with `--min-pt N`, content text under N (footer chrome — date,
//!   footer, slide number, fields, a single line in the bottom 12 % — exempt);
//! - **overrun**: text needing more height than its box at its size (md2star's
//!   character model: 0.5 em per character, 1.2 line spacing, 15 % slack), reported
//!   with the size that would fit — an observation, a failure only with `--strict`.
//!
//! - **low-contrast**: a run that states its own colour, under the WCAG ratio against
//!   what is behind its shape (3:1 for large type, 4.5:1 otherwise); see `contrast.rs`
//!   for how the backdrop and theme colours are resolved;
//!
//! A photo credit (a shape named [`crate::pptx::CREDIT_NAME`]) is small and over its
//! picture on purpose, and is exempt from the type and overlap checks; its contrast is
//! still judged.
//!
//! Output: human-readable lines, or JSON with `--json` — the same keys as `md2star
//! audit --json` (`slide`, `kind`, `detail`, `shape`, and `measured_pt` / `fits_pt` on
//! an overrun). Exit status: 0 for nothing but observations, 1 on a defect, 2 when the
//! deck cannot be read.

mod contrast;

pub use contrast::{
    contrast_ratio, relative_luminance, CONTRAST_LARGE_BOLD_PT, CONTRAST_LARGE_PT,
    CONTRAST_MIN_LARGE, CONTRAST_MIN_NORMAL,
};

use contrast::{backdrop_in, backdrops, ColouredRun, Fill, Palette};

use crate::error::{Error, Result};
use crate::pptx::template::{ph_of, shapes, xfrm_of, Package, Styles};
use crate::pptx::xml::Node;

/// Under this size a projected slide stops being readable.
pub const LEGIBLE_MIN_PT: f32 = 12.0;
/// How far past an edge a shape may sit, as a share of the canvas.
pub const BLEED_TOLERANCE: f64 = 0.01;
/// Shared area, as a share of the smaller box, above which two boxes collide.
pub const OVERLAP_TOLERANCE: f64 = 0.10;
/// Share of the canvas above which a picture is a backdrop.
pub const BACKDROP_SHARE: f64 = 0.8;
/// Share of the slide's height, from the bottom, where one line of text is chrome.
pub const CHROME_BAND: f64 = 0.12;
/// md2star's character model: mean advance as a fraction of the em.
pub const AVG_CHAR_WIDTH_EM: f32 = 0.5;
/// Line height as a multiple of the size.
pub const LINE_SPACING: f32 = 1.2;
/// Slack before an overrun is reported.
pub const FIT_SLACK: f32 = 1.15;
const EMU_PER_PT: f32 = 12_700.0;

/// What `audit` checks against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditOptions {
    /// `--min-pt N`: also fail on slide content set under N points.
    pub min_pt: Option<f32>,
    /// `--strict`: also fail on `overrun`.
    pub strict: bool,
}

/// The kind of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingKind {
    /// A shape lies (partly) outside the slide (`off-slide`).
    OffCanvas,
    /// Two text shapes overlap (`overlap`).
    Overlap,
    /// A picture covers text (`overlap`, as md2star names it).
    ImageOverText,
    /// An empty placeholder (`ghost-placeholder`).
    GhostPlaceholder,
    /// Text below the legibility floor (`illegible-type`).
    Unreadable,
    /// Text below `--min-pt` (`under-min-pt`).
    BelowMinPt,
    /// Text under the WCAG contrast floor against its backdrop (`low-contrast`).
    LowContrast,
    /// Text overruns its box (`overrun`): an observation unless `--strict`.
    Overrun,
}

impl FindingKind {
    /// md2star's name for the kind (the JSON `kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            FindingKind::OffCanvas => "off-slide",
            FindingKind::Overlap | FindingKind::ImageOverText => "overlap",
            FindingKind::GhostPlaceholder => "ghost-placeholder",
            FindingKind::Unreadable => "illegible-type",
            FindingKind::BelowMinPt => "under-min-pt",
            FindingKind::LowContrast => "low-contrast",
            FindingKind::Overrun => "overrun",
        }
    }

    /// Whether this kind fails the audit (`overrun` only with `strict`).
    pub fn is_defect(self, strict: bool) -> bool {
        strict || self != FindingKind::Overrun
    }
}

/// One problem on one slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// 1-based slide number.
    pub slide: usize,
    /// What is wrong.
    pub kind: FindingKind,
    /// The offending shape's name, when one shape owns the finding.
    pub shape: String,
    /// A sentence a human can act on, with the measurement in it.
    pub detail: String,
    /// An overrun's size, in points.
    pub measured_pt: Option<f32>,
    /// The size an overrun would fit at.
    pub fits_pt: Option<f32>,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shape = if self.shape.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.shape)
        };
        write!(
            f,
            "slide {}: {}{shape} — {}",
            self.slide,
            self.kind.as_str(),
            self.detail
        )
    }
}

/// The result of auditing a deck.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditReport {
    /// How many slides were read.
    pub slides: usize,
    /// Every finding, by slide then severity then shape.
    pub findings: Vec<Finding>,
}

impl AuditReport {
    /// Whether the deck fails: any defect (and an overrun under `strict`).
    pub fn fails(&self, strict: bool) -> bool {
        self.findings.iter().any(|f| f.kind.is_defect(strict))
    }

    /// One line: what the audit found, or that it found nothing (md2star's wording).
    pub fn summary(&self) -> String {
        if self.findings.is_empty() {
            return if self.slides > 0 {
                format!("layout audit: clean ({} slides)", self.slides)
            } else {
                "layout audit: clean".into()
            };
        }
        let mut kinds: Vec<&str> = Vec::new();
        let mut counts: Vec<usize> = Vec::new();
        for f in &self.findings {
            let k = f.kind.as_str();
            match kinds.iter().position(|x| *x == k) {
                Some(i) => counts[i] += 1,
                None => {
                    kinds.push(k);
                    counts.push(1);
                }
            }
        }
        let order = [
            "off-slide",
            "overlap",
            "ghost-placeholder",
            "illegible-type",
            "under-min-pt",
            "low-contrast",
            "overrun",
        ];
        let breakdown: Vec<String> = order
            .iter()
            .filter_map(|k| {
                kinds
                    .iter()
                    .position(|x| x == k)
                    .map(|i| format!("{} {k}", counts[i]))
            })
            .collect();
        let mut affected: Vec<usize> = self.findings.iter().map(|f| f.slide).collect();
        affected.dedup();
        let n = self.findings.len();
        let s = affected.len();
        format!(
            "layout audit: {n} finding{} on {s} slide{} ({})",
            if n == 1 { "" } else { "s" },
            if s == 1 { "" } else { "s" },
            breakdown.join(", ")
        )
    }

    /// The findings as JSON, in `md2star audit --json`'s shape (an array, two-space
    /// indent, `measured_pt` / `fits_pt` only on an overrun).
    pub fn to_json(&self) -> String {
        if self.findings.is_empty() {
            return "[]".into();
        }
        let mut s = String::from("[\n");
        for (i, f) in self.findings.iter().enumerate() {
            s.push_str("  {\n");
            s.push_str(&format!("    \"slide\": {},\n", f.slide));
            s.push_str(&format!("    \"kind\": {},\n", json_str(f.kind.as_str())));
            s.push_str(&format!("    \"detail\": {},\n", json_str(&f.detail)));
            s.push_str(&format!("    \"shape\": {}", json_str(&f.shape)));
            if let Some(m) = f.measured_pt {
                s.push_str(&format!(",\n    \"measured_pt\": {}", json_num(m)));
            }
            if let Some(m) = f.fits_pt {
                s.push_str(&format!(",\n    \"fits_pt\": {}", json_num(m)));
            }
            s.push_str("\n  }");
            if i + 1 < self.findings.len() {
                s.push(',');
            }
            s.push('\n');
        }
        s.push(']');
        s
    }
}

fn json_str(text: &str) -> String {
    let mut s = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

fn json_num(v: f32) -> String {
    format!("{:?}", f64::from(v))
}

/// `%g`-style: `18` for 18.0, `10.5` for 10.5.
fn g(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-6 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}

type Box4 = (i64, i64, i64, i64);

/// One shape as the audit sees it.
struct Seen {
    name: String,
    picture: bool,
    placeholder: Option<String>,
    frame: Option<Box4>,
    /// Paragraphs: (text, sizes of its non-empty runs).
    paras: Vec<(String, Vec<f32>)>,
    has_field: bool,
    text_bearing: bool,
    /// A table: its cells are laid out in a grid, so the character model's overrun
    /// estimate does not apply (md2star's audit does not read tables at all).
    table: bool,
    /// What the shape paints behind its own text and over what lies under it.
    fill: Fill,
    /// The runs that state their own colour.
    coloured: Vec<ColouredRun>,
}

fn name_of(shape: &Node) -> String {
    shape
        .children
        .iter()
        .find(|c| c.local().starts_with("nv"))
        .and_then(|nv| nv.children.iter().find(|c| c.local() == "cNvPr"))
        .and_then(|c| c.attr("name"))
        .unwrap_or_default()
        .to_string()
}

/// A group's child-to-slide mapping: `x' = ox + (x - cx) * sx`.
#[derive(Clone, Copy)]
struct Affine {
    ox: f64,
    oy: f64,
    cx: f64,
    cy: f64,
    sx: f64,
    sy: f64,
}

impl Affine {
    const ID: Affine = Affine {
        ox: 0.0,
        oy: 0.0,
        cx: 0.0,
        cy: 0.0,
        sx: 1.0,
        sy: 1.0,
    };

    fn apply(&self, b: Box4) -> Box4 {
        (
            (self.ox + (b.0 as f64 - self.cx) * self.sx).round() as i64,
            (self.oy + (b.1 as f64 - self.cy) * self.sy).round() as i64,
            (b.2 as f64 * self.sx).round() as i64,
            (b.3 as f64 * self.sy).round() as i64,
        )
    }

    fn then_group(&self, group: &Node) -> Affine {
        let Some(x) = group.child("grpSpPr").and_then(|p| p.child("xfrm")) else {
            return *self;
        };
        let get = |el: &str, a: &str| x.child(el).and_then(|n| n.attr_i64(a)).unwrap_or(0) as f64;
        let (off_x, off_y, ext_x, ext_y) = (
            get("off", "x"),
            get("off", "y"),
            get("ext", "cx"),
            get("ext", "cy"),
        );
        let (ch_x, ch_y, ch_w, ch_h) = (
            get("chOff", "x"),
            get("chOff", "y"),
            get("chExt", "cx"),
            get("chExt", "cy"),
        );
        let sx = if ch_w > 0.0 { ext_x / ch_w } else { 1.0 };
        let sy = if ch_h > 0.0 { ext_y / ch_h } else { 1.0 };
        // Compose: inner child → group space → outer space.
        Affine {
            ox: self.ox + (off_x - self.cx) * self.sx,
            oy: self.oy + (off_y - self.cy) * self.sy,
            cx: ch_x,
            cy: ch_y,
            sx: sx * self.sx,
            sy: sy * self.sy,
        }
    }
}

fn collect(
    tree: &[&Node],
    affine: Affine,
    styles: &Styles,
    layout: &Node,
    palette: &Palette,
    out: &mut Vec<Seen>,
) {
    for shape in tree {
        match shape.local() {
            // An Office-2010 extension (an equation): read the shape the producer
            // meant, the first choice.
            "AlternateContent" => {
                if let Some(choice) = shape.children.iter().find(|c| c.local() == "Choice") {
                    let kids: Vec<&Node> = choice.children.iter().collect();
                    collect(&kids, affine, styles, layout, palette, out);
                }
            }
            "grpSp" => {
                let inner = affine.then_group(shape);
                let kids: Vec<&Node> = shape.children.iter().collect();
                collect(&kids, inner, styles, layout, palette, out);
            }
            "sp" | "pic" | "graphicFrame" | "cxnSp" => {
                let ph = ph_of(shape);
                let resolved = ph
                    .as_ref()
                    .map(|p| styles.resolve_on_slide(shape, layout, p));
                let frame = xfrm_of(shape)
                    .or_else(|| resolved.as_ref().and_then(|r| r.frame))
                    .map(|b| affine.apply(b));
                let mut paras = Vec::new();
                let mut coloured = Vec::new();
                let mut has_field = false;
                let list = shape.path(&["txBody", "lstStyle"]);
                let size_at = |level: usize| -> f32 {
                    let own = list
                        .and_then(|l| l.child(&format!("lvl{}pPr", level + 1)))
                        .and_then(|p| p.child("defRPr"))
                        .and_then(|r| r.attr("sz"))
                        .and_then(|v| v.parse::<u32>().ok());
                    let hundredths = own.unwrap_or_else(|| match &resolved {
                        Some(r) => r.sizes[level.min(8)],
                        None => styles.other_size(list, level),
                    });
                    hundredths as f32 / 100.0
                };
                let mut bodies: Vec<&Node> = Vec::new();
                if let Some(tx) = shape.child("txBody") {
                    bodies.push(tx);
                }
                if shape.local() == "graphicFrame" {
                    for tc in shape.descendants("tc") {
                        if let Some(tx) = tc.child("txBody") {
                            bodies.push(tx);
                        }
                    }
                }
                for tx in bodies {
                    for p in tx.children_named("p") {
                        let ppr = p.child("pPr");
                        let level = ppr
                            .and_then(|x| x.attr("lvl"))
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let para_sz = ppr
                            .and_then(|x| x.child("defRPr"))
                            .and_then(|r| r.attr("sz"))
                            .and_then(|v| v.parse::<u32>().ok())
                            .map(|v| v as f32 / 100.0);
                        let mut text = String::new();
                        let mut sizes = Vec::new();
                        for r in &p.children {
                            match r.local() {
                                "r" | "fld" => {
                                    if r.local() == "fld" {
                                        has_field = true;
                                    }
                                    let t =
                                        r.child("t").map(|t| t.text.clone()).unwrap_or_default();
                                    let sz = r
                                        .child("rPr")
                                        .and_then(|x| x.attr("sz"))
                                        .and_then(|v| v.parse::<u32>().ok())
                                        .map(|v| v as f32 / 100.0)
                                        .or(para_sz)
                                        .unwrap_or_else(|| size_at(level));
                                    if !t.trim().is_empty() {
                                        sizes.push(sz);
                                        let rpr = r.child("rPr");
                                        let own = rpr
                                            .and_then(|x| x.child("solidFill"))
                                            .and_then(|f| f.children.first())
                                            .and_then(|c| palette.colour(c, None));
                                        if let Some(rgb) = own {
                                            coloured.push(ColouredRun {
                                                rgb,
                                                size_pt: sz,
                                                bold: rpr.is_some_and(|x| {
                                                    matches!(x.attr("b"), Some("1" | "true"))
                                                }),
                                            });
                                        }
                                    }
                                    text.push_str(&t);
                                }
                                "br" => text.push('\n'),
                                // An equation (`a14:m`): its characters count as text, at
                                // the paragraph's size.
                                "m" => {
                                    let t: String = r
                                        .descendants("t")
                                        .iter()
                                        .map(|t| t.text.as_str())
                                        .collect();
                                    if !t.trim().is_empty() {
                                        let own = r
                                            .descendants("rPr")
                                            .iter()
                                            .filter_map(|x| x.attr("sz"))
                                            .filter_map(|v| v.parse::<u32>().ok())
                                            .map(|v| v as f32 / 100.0)
                                            .fold(None, |m: Option<f32>, v| {
                                                Some(m.map_or(v, |m| m.min(v)))
                                            });
                                        sizes.push(
                                            own.or(para_sz).unwrap_or_else(|| size_at(level)),
                                        );
                                    }
                                    text.push_str(&t);
                                }
                                _ => {}
                            }
                        }
                        paras.push((text, sizes));
                    }
                }
                let text_bearing =
                    shape.local() != "pic" && paras.iter().any(|(t, _)| !t.trim().is_empty());
                out.push(Seen {
                    name: name_of(shape),
                    picture: shape.local() == "pic",
                    placeholder: ph.map(|p| p.kind),
                    frame,
                    paras,
                    has_field,
                    text_bearing,
                    table: shape.local() == "graphicFrame",
                    fill: palette.shape_fill(shape),
                    coloured,
                });
            }
            _ => {}
        }
    }
}

fn intersection(a: Box4, b: Box4) -> f64 {
    let dx = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let dy = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    if dx > 0 && dy > 0 {
        dx as f64 * dy as f64
    } else {
        0.0
    }
}

fn area(b: Box4) -> f64 {
    b.2.max(0) as f64 * b.3.max(0) as f64
}

/// A one-glyph ornament (the "01" of a step, an arrow): not text that can collide.
fn is_ornament(text: &str) -> bool {
    let t = text.trim();
    let n = t.chars().count();
    if n == 0 || n > 3 {
        return false;
    }
    let letters: Vec<bool> = t.chars().map(char::is_alphabetic).collect();
    !letters.windows(2).any(|w| w[0] && w[1])
}

fn is_latent(kind: &Option<String>) -> bool {
    matches!(kind.as_deref(), Some("dt" | "ftr" | "sldNum" | "hdr"))
}

/// Audit a `.pptx` given as bytes.
///
/// # Errors
///
/// [`Error::Read`] when the bytes are not a readable `.pptx`.
///
/// # Examples
///
/// ```
/// use md2star_rs::audit::{audit, AuditOptions};
/// let deck = md2star_rs::convert("# Hello\n\n- one\n- two\n", md2star_rs::Format::Pptx,
///     &md2star_rs::Options::default()).unwrap();
/// let report = audit(&deck.bytes, &AuditOptions::default()).unwrap();
/// assert_eq!(report.slides, 1);
/// assert!(report.findings.is_empty(), "{:?}", report.findings);
/// ```
pub fn audit(pptx: &[u8], options: &AuditOptions) -> Result<AuditReport> {
    let package = Package::open(pptx)?;
    let pres = package.xml("ppt/presentation.xml")?;
    let canvas = pres
        .child("sldSz")
        .and_then(|s| Some((s.attr_i64("cx")?, s.attr_i64("cy")?)))
        .unwrap_or((12_192_000, 6_858_000));
    let rels = package.rels("ppt/presentation.xml");
    let slide_parts: Vec<String> = pres
        .child("sldIdLst")
        .map(|l| {
            l.children_named("sldId")
                .filter_map(|s| {
                    let rid = s.attrs.iter().find(|(k, _)| k.ends_with(":id"))?.1.clone();
                    rels.iter().find(|r| r.id == rid).map(|r| r.target.clone())
                })
                .collect()
        })
        .unwrap_or_default();
    let mut findings = Vec::new();
    for (k, part) in slide_parts.iter().enumerate() {
        let index = k + 1;
        let slide = package.xml(part)?;
        let layout_part = package
            .rel_of(part, "slideLayout")
            .ok_or_else(|| Error::Read(format!("{part} has no layout")))?;
        let layout = package.xml(&layout_part)?;
        let master_part = package
            .rel_of(&layout_part, "slideMaster")
            .ok_or_else(|| Error::Read(format!("{layout_part} has no master")))?;
        let styles = Styles::for_master(&package, &master_part)?;
        let master_palette = Palette::for_master(&package, &master_part, &styles.master);
        let layout_palette = master_palette.overridden(&layout);
        let slide_palette = layout_palette.overridden(&slide);
        let behind = Behind {
            slide_bg: slide_palette.background(&slide),
            layout_shapes: backdrops(&layout, &layout_palette),
            layout_bg: layout_palette.background(&layout),
            master_shapes: backdrops(&styles.master, &master_palette),
            master_bg: master_palette.background(&styles.master),
        };
        let mut seen = Vec::new();
        collect(
            &shapes(&slide),
            Affine::ID,
            &styles,
            &layout,
            &slide_palette,
            &mut seen,
        );
        audit_slide(index, &seen, canvas, options, &behind, &mut findings);
    }
    findings.sort_by(|a, b| {
        a.slide
            .cmp(&b.slide)
            .then(severity(a.kind).cmp(&severity(b.kind)))
            .then(a.shape.cmp(&b.shape))
    });
    Ok(AuditReport {
        slides: slide_parts.len(),
        findings,
    })
}

fn severity(kind: FindingKind) -> u8 {
    match kind {
        FindingKind::OffCanvas => 0,
        FindingKind::Overlap | FindingKind::ImageOverText => 1,
        FindingKind::GhostPlaceholder => 2,
        FindingKind::Unreadable => 3,
        FindingKind::BelowMinPt => 4,
        FindingKind::LowContrast => 5,
        FindingKind::Overrun => 6,
    }
}

fn finding(slide: usize, kind: FindingKind, shape: &str, detail: String) -> Finding {
    Finding {
        slide,
        kind,
        shape: shape.to_string(),
        detail,
        measured_pt: None,
        fits_pt: None,
    }
}

/// What lies behind a slide's own shapes, back to front.
struct Behind {
    slide_bg: Fill,
    layout_shapes: Vec<(Box4, Fill)>,
    layout_bg: Fill,
    master_shapes: Vec<(Box4, Fill)>,
    master_bg: Fill,
}

impl Behind {
    /// The colour a reader sees behind shape `i` of `seen`, resolved as md2star does:
    /// its own fill, the shapes under it, the slide's background, the layout's shapes
    /// and background, the master's. `None` when nothing is there or it is no single
    /// colour.
    fn colour_behind(&self, seen: &[Seen], i: usize, bx: Box4) -> Option<contrast::Rgb> {
        let under: Vec<(Box4, Fill)> = seen[..i]
            .iter()
            .filter_map(|s| s.frame.map(|f| (f, s.fill)))
            .collect();
        let chain = [
            Some(seen[i].fill).filter(|f| *f != Fill::None),
            backdrop_in(&under, bx),
            Some(self.slide_bg).filter(|f| *f != Fill::None),
            backdrop_in(&self.layout_shapes, bx),
            Some(self.layout_bg).filter(|f| *f != Fill::None),
            backdrop_in(&self.master_shapes, bx),
            Some(self.master_bg).filter(|f| *f != Fill::None),
        ];
        match chain.into_iter().flatten().next()? {
            Fill::Solid(rgb) => Some(rgb),
            Fill::None | Fill::Opaque => None,
        }
    }
}

fn audit_slide(
    index: usize,
    seen: &[Seen],
    canvas: (i64, i64),
    options: &AuditOptions,
    behind: &Behind,
    out: &mut Vec<Finding>,
) {
    let (w, h) = canvas;
    let mut texts: Vec<(&Seen, Box4)> = Vec::new();
    let mut pictures: Vec<(&Seen, Box4)> = Vec::new();
    for (i, s) in seen.iter().enumerate() {
        if let Some(b) = s.frame.filter(|b| b.2 > 0 && b.3 > 0) {
            let (tx, ty) = (w as f64 * BLEED_TOLERANCE, h as f64 * BLEED_TOLERANCE);
            let mut over = Vec::new();
            let pt = |emu: i64| format!("{:.0}pt", emu as f32 / EMU_PER_PT);
            if (b.0 as f64) < -tx {
                over.push(format!("{} past the left edge", pt(-b.0)));
            }
            if (b.1 as f64) < -ty {
                over.push(format!("{} past the top edge", pt(-b.1)));
            }
            if (b.0 + b.2) as f64 > w as f64 + tx {
                over.push(format!("{} past the right edge", pt(b.0 + b.2 - w)));
            }
            if (b.1 + b.3) as f64 > h as f64 + ty {
                over.push(format!("{} past the bottom edge", pt(b.1 + b.3 - h)));
            }
            if !over.is_empty() {
                out.push(finding(
                    index,
                    FindingKind::OffCanvas,
                    &s.name,
                    over.join(", "),
                ));
            }
            if s.picture {
                pictures.push((s, b));
            }
        }
        if s.picture {
            continue;
        }
        if !s.text_bearing {
            if s.placeholder.is_some() && !is_latent(&s.placeholder) {
                out.push(finding(
                    index,
                    FindingKind::GhostPlaceholder,
                    &s.name,
                    "unfilled placeholder".into(),
                ));
            }
            continue;
        }
        if let Some(b) = s.frame.filter(|b| b.2 > 0 && b.3 > 0) {
            if !s.coloured.is_empty() {
                if let Some(bg) = behind.colour_behind(seen, i, b) {
                    if let Some((ratio, floor, rgb)) = contrast::worst(&s.coloured, bg) {
                        out.push(finding(
                            index,
                            FindingKind::LowContrast,
                            &s.name,
                            format!(
                                "#{} on #{} is {ratio:.1}:1, under the {}:1 floor",
                                contrast::hex(rgb),
                                contrast::hex(bg),
                                g(floor as f32)
                            ),
                        ));
                    }
                }
            }
        }
        if s.name == crate::pptx::CREDIT_NAME {
            continue;
        }
        let mut sizes: Vec<f32> = s
            .paras
            .iter()
            .flat_map(|(_, z)| z.iter().copied())
            .collect();
        sizes.sort_by(f32::total_cmp);
        sizes.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let tiny: Vec<String> = sizes
            .iter()
            .filter(|&&p| p < LEGIBLE_MIN_PT)
            .map(|&p| format!("{}pt", g(p)))
            .collect();
        if !tiny.is_empty() {
            out.push(finding(
                index,
                FindingKind::Unreadable,
                &s.name,
                format!(
                    "{} is under the {}pt legibility floor",
                    tiny.join(", "),
                    g(LEGIBLE_MIN_PT)
                ),
            ));
        }
        if let Some(min) = options.min_pt {
            let chrome = is_latent(&s.placeholder) || s.has_field || {
                let lines = s.paras.iter().filter(|(t, _)| !t.trim().is_empty()).count();
                let single = lines <= 1 && !s.paras.iter().any(|(t, _)| t.trim().contains('\n'));
                single
                    && s.frame
                        .is_some_and(|b| b.1 as f64 >= h as f64 * (1.0 - CHROME_BAND))
            };
            if !chrome {
                let small: Vec<String> = sizes
                    .iter()
                    .filter(|&&p| p < min - 0.001)
                    .map(|&p| format!("{}pt", g(p)))
                    .collect();
                if !small.is_empty() {
                    out.push(finding(
                        index,
                        FindingKind::BelowMinPt,
                        &s.name,
                        format!("{} is under the {}pt minimum", small.join(", "), g(min)),
                    ));
                }
            }
        }
        if let Some(b) = s.frame.filter(|b| b.2 > 0 && b.3 > 0 && !s.table) {
            let width_pt = b.2 as f32 / EMU_PER_PT;
            let mut needed = 0.0f32;
            let mut largest = 0.0f32;
            for (text, sizes) in &s.paras {
                if text.trim().is_empty() || sizes.is_empty() {
                    continue;
                }
                let pt = sizes.iter().copied().fold(0.0f32, f32::max);
                largest = largest.max(pt);
                let cpl = (width_pt / (pt * AVG_CHAR_WIDTH_EM)).floor().max(1.0);
                for line in text.split('\n') {
                    let n = line.chars().count() as f32;
                    needed += (n / cpl).ceil().max(1.0) * pt * LINE_SPACING;
                }
            }
            let available = b.3 as f32 / EMU_PER_PT;
            if needed > 0.0 && available > 0.0 && needed > available * FIT_SLACK {
                let implied = largest * (available / needed).sqrt();
                out.push(Finding {
                    slide: index,
                    kind: FindingKind::Overrun,
                    shape: s.name.clone(),
                    detail: format!(
                        "text needs {:.1}x its box at {}pt (fits at ~{:.0}pt)",
                        needed / available,
                        g(largest),
                        implied
                    ),
                    measured_pt: Some(largest),
                    fits_pt: Some(implied),
                });
            }
        }
        if let Some(b) = s.frame.filter(|b| b.2 > 0 && b.3 > 0) {
            let all: String = s.paras.iter().map(|(t, _)| t.as_str()).collect();
            if !is_ornament(&all) {
                texts.push((s, b));
            }
        }
    }
    for (i, (a, ba)) in texts.iter().enumerate() {
        for (b, bb) in &texts[i + 1..] {
            let shared = intersection(*ba, *bb);
            if shared == 0.0 {
                continue;
            }
            let smaller = area(*ba).min(area(*bb));
            let fraction = if smaller > 0.0 { shared / smaller } else { 0.0 };
            if fraction > OVERLAP_TOLERANCE {
                out.push(finding(
                    index,
                    FindingKind::Overlap,
                    "",
                    format!(
                        "'{}' and '{}' share {:.0}% of the smaller box",
                        a.name,
                        b.name,
                        fraction * 100.0
                    ),
                ));
            }
        }
    }
    let limit = w as f64 * h as f64 * BACKDROP_SHARE;
    for (p, pb) in &pictures {
        if area(*pb) >= limit {
            continue;
        }
        for (t, tb) in &texts {
            let shared = intersection(*pb, *tb);
            if shared == 0.0 {
                continue;
            }
            let smaller = area(*pb).min(area(*tb));
            let fraction = if smaller > 0.0 { shared / smaller } else { 0.0 };
            if fraction > OVERLAP_TOLERANCE {
                out.push(finding(
                    index,
                    FindingKind::ImageOverText,
                    &p.name,
                    format!(
                        "picture '{}' covers text '{}' ({:.0}% of the smaller box)",
                        p.name,
                        t.name,
                        fraction * 100.0
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_keep_md2stars_names() {
        assert_eq!(FindingKind::OffCanvas.as_str(), "off-slide");
        assert_eq!(FindingKind::ImageOverText.as_str(), "overlap");
        assert_eq!(FindingKind::BelowMinPt.as_str(), "under-min-pt");
        assert!(!FindingKind::Overrun.is_defect(false));
        assert!(FindingKind::Overrun.is_defect(true));
    }

    #[test]
    fn the_summary_reads_like_md2stars() {
        let clean = AuditReport {
            slides: 12,
            findings: vec![],
        };
        assert_eq!(clean.summary(), "layout audit: clean (12 slides)");
        let one = AuditReport {
            slides: 3,
            findings: vec![finding(3, FindingKind::Overlap, "", "x".into())],
        };
        assert_eq!(
            one.summary(),
            "layout audit: 1 finding on 1 slide (1 overlap)"
        );
        assert!(one.fails(false));
    }

    #[test]
    fn json_is_md2stars_shape() {
        let r = AuditReport {
            slides: 1,
            findings: vec![Finding {
                slide: 1,
                kind: FindingKind::Overrun,
                shape: "Subtitle 2".into(),
                detail: "text \"needs\" more".into(),
                measured_pt: Some(20.0),
                fits_pt: Some(12.5),
            }],
        };
        let j = r.to_json();
        assert!(j.starts_with("[\n  {\n    \"slide\": 1,\n    \"kind\": \"overrun\","));
        assert!(j.contains("\"detail\": \"text \\\"needs\\\" more\""));
        assert!(j.contains("\"measured_pt\": 20.0"));
        assert!(j.contains("\"fits_pt\": 12.5"));
        assert_eq!(AuditReport::default().to_json(), "[]");
    }

    #[test]
    fn ornaments_are_not_text() {
        assert!(is_ornament("01"));
        assert!(is_ornament("→"));
        assert!(!is_ornament("Hi there"));
        assert!(!is_ornament("Ab"));
    }

    #[test]
    fn g_formats_like_python() {
        assert_eq!(g(18.0), "18");
        assert_eq!(g(10.5), "10.5");
    }

    #[test]
    fn not_a_deck_is_a_read_error() {
        assert!(matches!(
            audit(b"PK nope", &AuditOptions::default()),
            Err(Error::Read(_))
        ));
    }
}
