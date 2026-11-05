//! PowerPoint templates: reading a `.pptx`'s masters, layouts and theme.
//!
//! Pure XML over the zip, no Office, no Python. [`inspect`] answers what a template
//! offers — its slide size, its layouts by name with every placeholder's type, index,
//! box and declared text sizes, **resolved** the way a renderer resolves them: a
//! placeholder that declares no box takes its master's, a paragraph level that declares
//! no size takes the layout's, then the master placeholder's, then the master's
//! `titleStyle` / `bodyStyle` / `otherStyle`, then the presentation's default text style.
//! [`crate::pptx::layout`] builds its catalogue from that, and the writer builds slides
//! *on* those layouts — the template's own masters, theme and layouts are kept, never
//! replaced by a generic scaffold.
//!
//! The same resolution serves [`crate::audit`], which reads any deck, not only ours.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use super::xml::{self, Node};
use crate::error::{Error, Result};

/// One placeholder of a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    /// The placeholder type (`title`, `ctrTitle`, `body`, `pic`, `subTitle`, …). A
    /// `<p:ph>` without a `type` is a content placeholder and reads `body`.
    pub kind: String,
    /// Its `idx`.
    pub idx: u32,
    /// Its box, `(x, y, cx, cy)` in EMU, when the layout (or its master) declares one.
    pub frame: Option<(i64, i64, i64, i64)>,
    /// The shape's name in the layout (`Content Placeholder 2`).
    pub name: String,
    /// `orient="vert"`: text set vertically. Never chosen for prose automatically.
    pub vertical: bool,
    /// The resolved text size of each list level 1–9, in hundredths of a point.
    pub sizes: [u32; 9],
    /// Whether level-1 text is bold by inheritance.
    pub bold: bool,
    /// The resolved Latin typeface (theme fonts substituted), when one is declared.
    pub typeface: Option<String>,
    /// Text insets `(left, top, right, bottom)` in EMU.
    pub insets: (i64, i64, i64, i64),
    /// Each list level's left margin and first-line indent (`marL`, `indent`), EMU.
    pub margins: [(i64, i64); 9],
}

impl Placeholder {
    /// The level-`level` (0-based) text size in points.
    pub fn size_pt(&self, level: usize) -> f32 {
        self.sizes[level.min(8)] as f32 / 100.0
    }

    /// Whether this placeholder holds a title.
    pub fn is_title(&self) -> bool {
        matches!(self.kind.as_str(), "title" | "ctrTitle")
    }

    /// Whether this placeholder takes running text (body, subtitle, content).
    pub fn is_body(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "body" | "subTitle" | "obj" | "tbl" | "chart"
        )
    }

    /// Whether this placeholder takes a picture.
    pub fn is_picture(&self) -> bool {
        matches!(self.kind.as_str(), "pic" | "media" | "clipArt")
    }

    /// Date, footer and slide number: furniture PowerPoint fills itself.
    pub fn is_latent(&self) -> bool {
        matches!(self.kind.as_str(), "dt" | "ftr" | "sldNum" | "hdr")
    }
}

/// One named layout of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    /// The layout's name as PowerPoint shows it (`Title and Content`, `Big Number`…).
    pub name: String,
    /// The part path inside the zip (`ppt/slideLayouts/slideLayout2.xml`).
    pub part: String,
    /// Its placeholders, in the layout's order.
    pub placeholders: Vec<Placeholder>,
    /// How many of the template's own example slides use it: the designer's most used
    /// layout for a role is the canonical one.
    pub example_slides: usize,
    /// The boxes of the layout's own drawing (bands, pictures, rules): not
    /// placeholders, but nothing md2star-rs places should be grown over them.
    pub decorations: Vec<(i64, i64, i64, i64)>,
}

/// What a template offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInfo {
    /// Slide width and height in EMU.
    pub slide_size: (i64, i64),
    /// The layouts, in master order.
    pub layouts: Vec<LayoutInfo>,
    /// The theme's heading (major) and body (minor) Latin typefaces.
    pub theme_fonts: (Option<String>, Option<String>),
    /// The top of the footer band (the highest date / footer / slide-number box), in
    /// EMU: content placed by md2star-rs stays above it.
    pub footer_top: i64,
}

impl TemplateInfo {
    /// The layout named `name` (exact match).
    pub fn layout(&self, name: &str) -> Option<&LayoutInfo> {
        self.layouts.iter().find(|l| l.name == name)
    }
}

/// Read a template's slide size and layouts.
///
/// # Errors
///
/// [`Error::Template`] when the bytes are not a `.pptx` with a presentation part, a
/// master and at least one layout.
///
/// # Examples
///
/// ```
/// use md2star_rs::pptx::template::inspect;
/// let info = inspect(md2star_rs::templates::DEFAULT_PPTX).unwrap();
/// assert_eq!(info.slide_size, (12_192_000, 6_858_000));
/// assert!(info.layout("Title and Content").is_some());
/// ```
pub fn inspect(pptx: &[u8]) -> Result<TemplateInfo> {
    let package = Package::open(pptx).map_err(|e| Error::Template(e.to_string()))?;
    template_info(&package)
}

pub(crate) fn template_info(package: &Package) -> Result<TemplateInfo> {
    let pres = package
        .xml("ppt/presentation.xml")
        .map_err(|e| Error::Template(format!("no presentation part: {e}")))?;
    let slide_size = pres
        .child("sldSz")
        .and_then(|s| Some((s.attr_i64("cx")?, s.attr_i64("cy")?)))
        .unwrap_or((12_192_000, 6_858_000));
    let masters: Vec<String> = package
        .rels("ppt/presentation.xml")
        .into_iter()
        .filter(|r| r.kind.ends_with("/slideMaster"))
        .map(|r| r.target)
        .collect();
    if masters.is_empty() {
        return Err(Error::Template("the template has no slide master".into()));
    }
    let mut usage: BTreeMap<String, usize> = BTreeMap::new();
    for rel in package.rels("ppt/presentation.xml") {
        if rel.kind.ends_with("/slide") {
            if let Some(layout) = package.rel_of(&rel.target, "slideLayout") {
                *usage.entry(layout).or_default() += 1;
            }
        }
    }
    let mut layouts = Vec::new();
    let mut theme_fonts = (None, None);
    let mut footer_top = slide_size.1;
    let mut seen = std::collections::BTreeSet::new();
    for master_part in &masters {
        let styles =
            Styles::for_master(package, master_part).map_err(|e| Error::Template(e.to_string()))?;
        if theme_fonts.0.is_none() && theme_fonts.1.is_none() {
            theme_fonts = (styles.major.clone(), styles.minor.clone());
        }
        // Layouts in the master's own order (sldLayoutIdLst): PowerPoint's order.
        let rels = package.rels(master_part);
        let mut ordered = Vec::new();
        if let Some(list) = styles.master.child("sldLayoutIdLst") {
            for id in list.children_named("sldLayoutId") {
                let rid = id.attr("id").and_then(|_| {
                    id.attrs
                        .iter()
                        .find(|(k, _)| k.ends_with(":id"))
                        .map(|(_, v)| v.clone())
                });
                if let Some(rel) = rid.and_then(|rid| rels.iter().find(|r| r.id == rid)) {
                    ordered.push(rel.target.clone());
                }
            }
        }
        for rel in rels.iter().filter(|r| r.kind.ends_with("/slideLayout")) {
            if !ordered.contains(&rel.target) {
                ordered.push(rel.target.clone());
            }
        }
        for part in ordered {
            if !seen.insert(part.clone()) {
                continue;
            }
            let Ok(layout) = package.xml(&part) else {
                continue;
            };
            let name = layout
                .child("cSld")
                .and_then(|c| c.attr("name"))
                .map(str::to_string)
                .unwrap_or_else(|| stem(&part));
            let mut placeholders = Vec::new();
            let mut decorations = Vec::new();
            for sp in shapes(&layout) {
                let Some(ph) = ph_of(sp) else {
                    if let Some(f) = xfrm_of(sp) {
                        decorations.push(f);
                    }
                    continue;
                };
                let p = styles.resolve(None, Some(sp), &ph);
                if p.is_latent() {
                    if let Some((_, y, _, _)) = p.frame {
                        footer_top = footer_top.min(y);
                    }
                }
                placeholders.push(p);
            }
            layouts.push(LayoutInfo {
                name,
                example_slides: usage.get(&part).copied().unwrap_or(0),
                decorations,
                part,
                placeholders,
            });
        }
    }
    if layouts.is_empty() {
        return Err(Error::Template("the template has no slide layout".into()));
    }
    if footer_top <= slide_size.1 / 2 {
        footer_top = slide_size.1 * 92 / 100;
    }
    Ok(TemplateInfo {
        slide_size,
        layouts,
        theme_fonts,
        footer_top,
    })
}

fn stem(part: &str) -> String {
    let file = part.rsplit('/').next().unwrap_or(part);
    file.strip_suffix(".xml").unwrap_or(file).to_string()
}

// ── the package ─────────────────────────────────────────────────────────────

/// A relationship of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rel {
    pub id: String,
    pub kind: String,
    /// Resolved to a part path (`ppt/slideLayouts/slideLayout1.xml`), unless external.
    pub target: String,
    pub external: bool,
}

/// An OPC package read into memory: every part's bytes by path.
#[derive(Debug, Clone, Default)]
pub(crate) struct Package {
    pub parts: BTreeMap<String, Vec<u8>>,
}

impl Package {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| Error::Read(format!("not a zip package: {e}")))?;
        let mut parts = BTreeMap::new();
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| Error::Read(format!("unreadable zip entry: {e}")))?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().trim_start_matches('/').to_string();
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .map_err(|e| Error::Read(format!("unreadable zip entry {name}: {e}")))?;
            parts.insert(name, data);
        }
        if !parts.contains_key("ppt/presentation.xml") {
            return Err(Error::Read(
                "not a PowerPoint package (no ppt/presentation.xml)".into(),
            ));
        }
        Ok(Package { parts })
    }

    pub fn xml(&self, part: &str) -> Result<Node> {
        let bytes = self
            .parts
            .get(part)
            .ok_or_else(|| Error::Read(format!("missing part {part}")))?;
        xml::parse(bytes).map_err(|e| Error::Read(format!("{part}: {e}")))
    }

    pub fn rels(&self, part: &str) -> Vec<Rel> {
        let (dir, file) = match part.rfind('/') {
            Some(i) => (&part[..i], &part[i + 1..]),
            None => ("", part),
        };
        let rels_part = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let Some(bytes) = self.parts.get(&rels_part) else {
            return Vec::new();
        };
        let Ok(root) = xml::parse(bytes) else {
            return Vec::new();
        };
        root.children_named("Relationship")
            .filter_map(|r| {
                let external = r.attr("TargetMode") == Some("External");
                let raw = r.attr("Target")?;
                Some(Rel {
                    id: r.attr("Id")?.to_string(),
                    kind: r.attr("Type").unwrap_or_default().to_string(),
                    target: if external {
                        raw.to_string()
                    } else {
                        join(dir, raw)
                    },
                    external,
                })
            })
            .collect()
    }

    /// The first relationship of `part` whose type ends with `/kind`.
    pub fn rel_of(&self, part: &str, kind: &str) -> Option<String> {
        let suffix = format!("/{kind}");
        self.rels(part)
            .into_iter()
            .find(|r| r.kind.ends_with(&suffix))
            .map(|r| r.target)
    }
}

/// Resolve a relationship target against the directory of its source part.
pub(crate) fn join(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            ".." => {
                segments.pop();
            }
            "." | "" => {}
            s => segments.push(s),
        }
    }
    segments.join("/")
}

// ── shapes and placeholders ─────────────────────────────────────────────────

/// The elements directly in a part's shape tree (`<p:sp>`, `<p:pic>`, …).
pub(crate) fn shapes(part: &Node) -> Vec<&Node> {
    part.path(&["cSld", "spTree"])
        .map(|t| t.children.iter().collect())
        .unwrap_or_default()
}

/// A shape's `<p:ph>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ph {
    pub kind: String,
    pub idx: u32,
    pub vertical: bool,
}

pub(crate) fn ph_of(shape: &Node) -> Option<Ph> {
    let nv = shape
        .children
        .iter()
        .find(|c| c.local().starts_with("nv"))?;
    let ph = nv.path(&["nvPr", "ph"])?;
    Some(Ph {
        kind: ph.attr("type").unwrap_or("body").to_string(),
        idx: ph.attr("idx").and_then(|v| v.parse().ok()).unwrap_or(0),
        vertical: ph.attr("orient").is_some_and(|o| o.starts_with("vert")),
    })
}

/// A shape's own `(x, y, cx, cy)` from its `<a:xfrm>` (`<p:xfrm>` for a graphic frame).
pub(crate) fn xfrm_of(shape: &Node) -> Option<(i64, i64, i64, i64)> {
    let xfrm = shape
        .children
        .iter()
        .find(|c| matches!(c.local(), "spPr" | "grpSpPr"))
        .and_then(|pr| pr.child("xfrm"))
        .or_else(|| shape.child("xfrm"))?;
    let off = xfrm.child("off")?;
    let ext = xfrm.child("ext")?;
    Some((
        off.attr_i64("x")?,
        off.attr_i64("y")?,
        ext.attr_i64("cx")?,
        ext.attr_i64("cy")?,
    ))
}

/// The master-level family a placeholder type belongs to.
fn master_kind(kind: &str) -> &str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => kind,
        _ => "body",
    }
}

fn find_ph<'a>(part: &'a Node, ph: &Ph, by_idx: bool) -> Option<&'a Node> {
    let candidates: Vec<(&Node, Ph)> = shapes(part)
        .into_iter()
        .filter_map(|s| ph_of(s).map(|p| (s, p)))
        .collect();
    if by_idx && ph.idx != 0 {
        if let Some((s, _)) = candidates.iter().find(|(_, p)| p.idx == ph.idx) {
            return Some(s);
        }
    }
    if let Some((s, _)) = candidates.iter().find(|(_, p)| p.kind == ph.kind) {
        return Some(s);
    }
    let family = master_kind(&ph.kind);
    candidates
        .iter()
        .find(|(_, p)| master_kind(&p.kind) == family)
        .map(|(s, _)| *s)
}

// ── text styles ─────────────────────────────────────────────────────────────

/// One paragraph level's declared properties (`<a:lvlNpPr>` / `<a:defRPr>`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Level {
    sz: Option<u32>,
    bold: Option<bool>,
    typeface: Option<String>,
    mar_l: Option<i64>,
    indent: Option<i64>,
}

fn levels_of(list: Option<&Node>) -> [Level; 9] {
    let mut out: [Level; 9] = Default::default();
    let Some(list) = list else { return out };
    for (i, slot) in out.iter_mut().enumerate() {
        let name = format!("lvl{}pPr", i + 1);
        let Some(p) = list.child(&name) else { continue };
        slot.mar_l = p.attr_i64("marL");
        slot.indent = p.attr_i64("indent");
        if let Some(r) = p.child("defRPr") {
            slot.sz = r.attr("sz").and_then(|v| v.parse().ok());
            slot.bold = r.attr("b").map(|b| b == "1" || b == "true");
            slot.typeface = r
                .child("latin")
                .and_then(|l| l.attr("typeface"))
                .map(str::to_string);
        }
    }
    out
}

fn body_pr_insets(shape: &Node) -> [Option<i64>; 4] {
    let mut out = [None; 4];
    if let Some(bp) = shape.path(&["txBody", "bodyPr"]) {
        for (i, k) in ["lIns", "tIns", "rIns", "bIns"].iter().enumerate() {
            out[i] = bp.attr_i64(k);
        }
    }
    out
}

/// The text-style inheritance chain of one master, its theme fonts and the
/// presentation's defaults: everything needed to resolve what a placeholder's text
/// renders at.
#[derive(Debug, Clone)]
pub(crate) struct Styles {
    pub master: Node,
    title: [Level; 9],
    body: [Level; 9],
    other: [Level; 9],
    default: [Level; 9],
    pub major: Option<String>,
    pub minor: Option<String>,
}

impl Styles {
    pub fn for_master(package: &Package, master_part: &str) -> Result<Self> {
        let master = package.xml(master_part)?;
        let tx = master.child("txStyles");
        let title = levels_of(tx.and_then(|t| t.child("titleStyle")));
        let body = levels_of(tx.and_then(|t| t.child("bodyStyle")));
        let other = levels_of(tx.and_then(|t| t.child("otherStyle")));
        let default = package
            .xml("ppt/presentation.xml")
            .ok()
            .map(|p| levels_of(p.child("defaultTextStyle")))
            .unwrap_or_default();
        let (mut major, mut minor) = (None, None);
        if let Some(theme_part) = package.rel_of(master_part, "theme") {
            if let Ok(theme) = package.xml(&theme_part) {
                if let Some(fs) = theme.descendants("fontScheme").first() {
                    let face = |which: &str| {
                        fs.child(which)
                            .and_then(|f| f.child("latin"))
                            .and_then(|l| l.attr("typeface"))
                            .filter(|t| !t.is_empty())
                            .map(str::to_string)
                    };
                    major = face("majorFont");
                    minor = face("minorFont");
                }
            }
        }
        Ok(Styles {
            master,
            title,
            body,
            other,
            default,
            major,
            minor,
        })
    }

    fn theme_face(&self, face: &str) -> Option<String> {
        if face.starts_with("+mj") {
            self.major.clone()
        } else if face.starts_with("+mn") {
            self.minor.clone()
        } else {
            Some(face.to_string())
        }
    }

    /// The text size (hundredths of a point) of a non-placeholder text shape at
    /// `level` (0-based) when neither the run nor the shape declares one.
    pub fn other_size(&self, list: Option<&Node>, level: usize) -> u32 {
        let own = levels_of(list);
        let level = level.min(8);
        own[level]
            .sz
            .or(self.other[level].sz)
            .or(self.default[level].sz)
            .unwrap_or(1800)
    }

    /// Resolve a placeholder: `slide` is the slide's shape (if any), `layout` the
    /// layout's matching shape; the master's is looked up by type.
    pub fn resolve(&self, slide: Option<&Node>, layout: Option<&Node>, ph: &Ph) -> Placeholder {
        let master = find_ph(&self.master, ph, false);
        let chain: Vec<&Node> = [slide, layout, master].into_iter().flatten().collect();
        let frame = chain.iter().find_map(|s| xfrm_of(s));
        let lists: Vec<[Level; 9]> = chain
            .iter()
            .map(|s| levels_of(s.path(&["txBody", "lstStyle"])))
            .collect();
        let family = match master_kind(&ph.kind) {
            "title" => &self.title,
            "body" => &self.body,
            _ => &self.other,
        };
        let mut sizes = [1800u32; 9];
        for (lvl, size) in sizes.iter_mut().enumerate() {
            *size = lists
                .iter()
                .find_map(|l| l[lvl].sz)
                .or(family[lvl].sz)
                .or(self.default[lvl].sz)
                .unwrap_or(1800);
        }
        let mut margins = [(0i64, 0i64); 9];
        for (lvl, m) in margins.iter_mut().enumerate() {
            let pick = |f: fn(&Level) -> Option<i64>| {
                lists
                    .iter()
                    .find_map(|l| f(&l[lvl]))
                    .or(f(&family[lvl]))
                    .or(f(&self.default[lvl]))
            };
            *m = (
                pick(|l| l.mar_l).unwrap_or(0),
                pick(|l| l.indent).unwrap_or(0),
            );
        }
        let bold = lists
            .iter()
            .find_map(|l| l[0].bold)
            .or(family[0].bold)
            .unwrap_or(false);
        let typeface = lists
            .iter()
            .find_map(|l| l[0].typeface.clone())
            .or_else(|| family[0].typeface.clone())
            .or_else(|| self.default[0].typeface.clone())
            .and_then(|f| self.theme_face(&f))
            .or_else(|| {
                if master_kind(&ph.kind) == "title" {
                    self.major.clone()
                } else {
                    self.minor.clone()
                }
            });
        let mut ins = [91_440, 45_720, 91_440, 45_720];
        for s in chain.iter().rev() {
            for (i, v) in body_pr_insets(s).into_iter().enumerate() {
                if let Some(v) = v {
                    ins[i] = v;
                }
            }
        }
        let name = chain
            .first()
            .and_then(|s| s.path(&["nvSpPr", "cNvPr"]))
            .and_then(|c| c.attr("name"))
            .unwrap_or_default()
            .to_string();
        Placeholder {
            kind: ph.kind.clone(),
            idx: ph.idx,
            frame,
            name,
            vertical: ph.vertical,
            sizes,
            bold,
            typeface,
            insets: (ins[0], ins[1], ins[2], ins[3]),
            margins,
        }
    }

    /// Resolve a placeholder shape of a slide whose layout part is `layout`.
    pub fn resolve_on_slide(&self, slide_shape: &Node, layout: &Node, ph: &Ph) -> Placeholder {
        let on_layout = find_ph(layout, ph, true);
        self.resolve(Some(slide_shape), on_layout, ph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &[u8] = include_bytes!("../../tests/fixtures/templates/template.pptx");

    #[test]
    fn reads_the_layouts_in_master_order() {
        let info = inspect(TEMPLATE).unwrap();
        let names: Vec<&str> = info.layouts.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names[0], "Title Slide");
        assert_eq!(names[1], "Title and Content");
        assert!(names.contains(&"Two Content"));
        assert_eq!(info.layouts.len(), 14);
        assert_eq!(
            info.theme_fonts,
            (Some("Century Gothic".into()), Some("Century Gothic".into()))
        );
    }

    #[test]
    fn a_placeholder_without_a_box_takes_its_masters() {
        let info = inspect(TEMPLATE).unwrap();
        let two = info.layout("Two Content").unwrap();
        let title = two.placeholders.iter().find(|p| p.is_title()).unwrap();
        // The layout declares no box for its title: the master's is used.
        assert_eq!(title.frame, Some((810_000, 447_188, 10_571_998, 970_450)));
        // The master's titleStyle sets 40 pt bold.
        assert_eq!(title.sizes[0], 4000);
        assert!(title.bold);
        let bodies: Vec<_> = two.placeholders.iter().filter(|p| p.is_body()).collect();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0].sizes[0], 1800);
        assert_eq!(bodies[0].sizes[1], 1600);
        assert_eq!(bodies[0].typeface.as_deref(), Some("Century Gothic"));
    }

    #[test]
    fn a_layout_size_wins_over_the_master_style() {
        let info = inspect(TEMPLATE).unwrap();
        let cover = info.layout("Title Slide").unwrap();
        let title = cover.placeholders.iter().find(|p| p.kind == "ctrTitle");
        assert_eq!(title.unwrap().sizes[0], 5400);
        assert!(info.footer_top < info.slide_size.1);
        assert!(info.footer_top > info.slide_size.1 / 2);
    }

    #[test]
    fn rejects_what_is_not_a_deck() {
        assert!(matches!(inspect(b"nope"), Err(Error::Template(_))));
    }

    #[test]
    fn joins_relative_targets() {
        assert_eq!(
            join("ppt/slides", "../slideLayouts/slideLayout1.xml"),
            "ppt/slideLayouts/slideLayout1.xml"
        );
        assert_eq!(join("", "ppt/presentation.xml"), "ppt/presentation.xml");
        assert_eq!(join("ppt/slides", "/ppt/media/a.png"), "ppt/media/a.png");
    }
}
