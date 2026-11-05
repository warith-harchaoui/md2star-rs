//! PPTX → Markdown (see [`crate::twin`] for what it must carry over).
//!
//! One section per slide, in `presentation.xml` order, separated by `---`:
//!
//! - the title placeholder becomes the heading — `#` for a section-header slide, `##`
//!   for the others (`#` throughout when the deck has no section slides, which is what
//!   Pandoc's slide-level inference expects);
//! - text bodies become paragraphs and bullets (a placeholder's paragraphs are bullets
//!   unless `a:buNone` says otherwise, a text box's are plain unless it declares a
//!   bullet), with their levels; consecutive all-monospace paragraphs become a code
//!   block;
//! - tables become pipe tables, pictures are extracted, two side-by-side bodies become
//!   a `:::: columns` div;
//! - speaker notes become `::: notes`;
//! - the cover slide (a centred title) becomes the front matter;
//! - Pandoc's footnotes (superscript numbers linking to a final "Notes" slide) come back
//!   as `[^n]` with their definitions.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::docx::{is_mono_name, media_path};
use super::md::{self, Fmt, Piece};
use super::xml::{Node, Package, Rel};
use super::{Twin, TwinOptions};
use crate::ast::{Align, Warning};
use crate::error::{Error, Result};

/// Read a `.pptx` back into Markdown.
///
/// # Errors
///
/// [`Error::Read`] when the bytes are not a readable `.pptx` (not a zip, no
/// `ppt/presentation.xml`, malformed XML).
pub fn to_markdown(pptx: &[u8], options: &TwinOptions) -> Result<Twin> {
    let pkg = Package::open(pptx)?;
    let pres_path = "ppt/presentation.xml";
    let Some(pres) = pkg.xml(pres_path)? else {
        return Err(if pkg.has("word/document.xml") {
            Error::Read("this is a Word document, not a deck; read it as .docx".into())
        } else {
            Error::Read("not a PowerPoint deck: no ppt/presentation.xml".into())
        });
    };
    let pres_rels = pkg.rels(pres_path)?;
    let slide_paths: Vec<String> = pres
        .child("sldIdLst")
        .map(|l| {
            l.children_named("sldId")
                .filter_map(|s| {
                    // `r:id` (the relationship), not the bare numeric `id`.
                    s.attrs
                        .iter()
                        .find(|(k, _)| k.contains(':') && k.ends_with(":id"))
                        .map(|(_, v)| v.clone())
                })
                .filter_map(|rid| pres_rels.get(&rid).map(|r| r.target.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut cx = Cx {
        pkg: &pkg,
        options,
        media: Vec::new(),
        media_by_part: HashMap::new(),
        used_names: HashSet::new(),
        warnings: Vec::new(),
        warned: HashSet::new(),
        rels: BTreeMap::new(),
        note_refs: Vec::new(),
        dropped_images: 0,
    };

    // Read every slide first: the heading depth depends on whether the deck has
    // section slides, and footnotes on whether a "Notes" slide is jumped to.
    let mut slides = Vec::new();
    for (i, path) in slide_paths.iter().enumerate() {
        let Some(root) = pkg.xml(path)? else {
            cx.warn_once(
                "twin-missing-slide",
                path,
                format!("slide part `{path}` is listed but missing"),
            );
            continue;
        };
        cx.rels = pkg.rels(path)?;
        let slide = cx.slide(i, path, &root)?;
        slides.push(slide);
    }

    // Pandoc's footnotes: superscript jumps to a final notes slide.
    let mut footnotes: Vec<String> = Vec::new();
    let jumped: HashSet<String> = cx.note_refs.iter().map(|(_, t)| t.clone()).collect();
    if let Some(pos) = slides.iter().position(|s| jumped.contains(&s.path)) {
        let notes_slide = slides.remove(pos);
        footnotes = numbered_notes(&notes_slide.body_paragraphs);
    }

    let has_sections = slides.iter().any(|s| s.section);
    let mut front = String::new();
    let mut parts: Vec<String> = Vec::new();
    for (i, slide) in slides.iter().enumerate() {
        if i == 0 && slide.cover {
            front = slide.front_matter();
            continue;
        }
        let mut blocks = Vec::new();
        if let Some(title) = &slide.title {
            let level = if slide.section || !has_sections { 1 } else { 2 };
            blocks.push(format!("{} {}", "#".repeat(level), title));
        }
        blocks.extend(slide.blocks.iter().cloned());
        if !slide.notes.is_empty() {
            blocks.push(format!("::: notes\n{}\n:::", slide.notes.join("\n\n")));
        }
        if blocks.is_empty() {
            continue;
        }
        parts.push(blocks.join("\n\n"));
    }
    let mut markdown = front;
    markdown.push_str(&parts.join("\n\n---\n\n"));
    if !footnotes.is_empty() {
        markdown.push_str("\n\n");
        let defs: Vec<String> = footnotes
            .iter()
            .enumerate()
            .map(|(i, text)| md::footnote(&(i + 1).to_string(), std::slice::from_ref(text)))
            .collect();
        markdown.push_str(&defs.join("\n\n"));
    }
    markdown.push('\n');
    if cx.dropped_images > 0 {
        cx.warnings.push(Warning::new(
            "twin-images-dropped",
            format!("{} image(s) left out (images disabled)", cx.dropped_images),
        ));
    }
    Ok(Twin {
        markdown,
        media: cx.media,
        warnings: cx.warnings,
    })
}

/// `1. text` paragraphs of a notes slide → the note texts, in number order.
fn numbered_notes(paragraphs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paragraphs {
        let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
        let rest = &p[digits.len()..];
        let text = match rest
            .strip_prefix("\\. ")
            .or_else(|| rest.strip_prefix(". "))
        {
            Some(t) if !digits.is_empty() => t,
            _ => p.as_str(),
        };
        out.push(text.trim().to_string());
    }
    out
}

/// What one slide gave.
struct Slide {
    path: String,
    title: Option<String>,
    /// A section-header slide (`#`).
    section: bool,
    /// The cover: a centred title and maybe a subtitle, nothing else.
    cover: bool,
    subtitle_lines: Vec<String>,
    blocks: Vec<String>,
    /// The body's paragraphs as plain Markdown lines (for a notes slide).
    body_paragraphs: Vec<String>,
    notes: Vec<String>,
}

impl Slide {
    fn front_matter(&self) -> String {
        let mut lines = Vec::new();
        if let Some(t) = &self.title {
            lines.push(format!("title: {}", md::yaml_scalar(t)));
        }
        let mut subtitle = Vec::new();
        let mut author = None;
        let mut date = None;
        for l in &self.subtitle_lines {
            let digits = l.chars().filter(char::is_ascii_digit).count();
            if date.is_none() && digits >= 4 {
                date = Some(l.clone());
            } else if author.is_none()
                && !l.chars().any(|c| c.is_ascii_digit())
                && l.split_whitespace().count() <= 6
                && l.split_whitespace()
                    .all(|w| w.chars().next().is_some_and(char::is_uppercase))
            {
                author = Some(l.clone());
            } else {
                subtitle.push(l.clone());
            }
        }
        if !subtitle.is_empty() {
            lines.push(format!(
                "subtitle: {}",
                md::yaml_scalar(&subtitle.join(" "))
            ));
        }
        if let Some(a) = author {
            lines.push(format!("author: {}", md::yaml_scalar(&a)));
        }
        if let Some(d) = date {
            lines.push(format!("date: {}", md::yaml_scalar(&d)));
        }
        if lines.is_empty() {
            String::new()
        } else {
            format!("---\n{}\n---\n\n", lines.join("\n"))
        }
    }
}

/// A drawable on a slide, flattened out of groups.
struct Shape<'a> {
    node: &'a Node,
    ph_type: Option<String>,
    is_placeholder: bool,
    pos: Option<(i64, i64, i64, i64)>,
}

struct Cx<'a> {
    pkg: &'a Package,
    options: &'a TwinOptions,
    media: Vec<(String, Vec<u8>)>,
    media_by_part: HashMap<String, String>,
    used_names: HashSet<String>,
    warnings: Vec<Warning>,
    warned: HashSet<String>,
    /// The relationships of the part being read.
    rels: BTreeMap<String, Rel>,
    /// (note number shown, target slide path) of every footnote-style jump.
    note_refs: Vec<(String, String)>,
    dropped_images: usize,
}

/// One paragraph of a text body.
struct Para {
    level: usize,
    bullet: Bullet,
    text: String,
    mono: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bullet {
    None,
    Unordered,
    Ordered,
}

impl Cx<'_> {
    fn warn_once(&mut self, code: &'static str, key: &str, message: String) {
        if self.warned.insert(format!("{code}:{key}")) {
            self.warnings.push(Warning::new(code, message));
        }
    }

    fn slide(&mut self, index: usize, path: &str, root: &Node) -> Result<Slide> {
        let layout_type = self
            .rels
            .values()
            .find(|r| r.kind == "slideLayout")
            .map(|r| r.target.clone())
            .and_then(|p| self.pkg.xml(&p).ok().flatten())
            .and_then(|l| l.attr("type").map(str::to_string))
            .unwrap_or_default();
        let notes_part = self
            .rels
            .values()
            .find(|r| r.kind == "notesSlide")
            .map(|r| r.target.clone());
        let mut shapes = Vec::new();
        if let Some(tree) = root.path(&["cSld", "spTree"]) {
            collect(tree, &mut shapes);
        }
        shapes.retain(|s| {
            !matches!(
                s.ph_type.as_deref(),
                Some("dt") | Some("ftr") | Some("sldNum") | Some("hdr") | Some("sldImg")
            )
        });
        let is_title = |s: &Shape| matches!(s.ph_type.as_deref(), Some("title") | Some("ctrTitle"));
        // A title placeholder, or — in decks drawn with plain text boxes — a box named
        // "Title", or a lone large line at the top.
        let title_shape = shapes
            .iter()
            .position(is_title)
            .or_else(|| shapes.iter().position(looks_like_title_box));
        let mut title = None;
        let mut centred = false;
        if let Some(i) = title_shape {
            let t = shapes.remove(i);
            centred = t.ph_type.as_deref() == Some("ctrTitle");
            let lines = self.title_lines(t.node);
            let joined = lines.join(" ").trim().to_string();
            if !joined.is_empty() {
                title = Some(joined);
            }
        }
        let mut subtitle_lines = Vec::new();
        if let Some(i) = shapes
            .iter()
            .position(|s| s.ph_type.as_deref() == Some("subTitle"))
        {
            let s = shapes.remove(i);
            subtitle_lines = self
                .text_lines(s.node)
                .into_iter()
                .filter(|l| !l.trim().is_empty())
                .collect();
        }
        // Reading order: top to bottom, then left to right, when every shape is placed.
        if shapes.iter().all(|s| s.pos.is_some()) {
            shapes.sort_by_key(|s| s.pos.map(|(x, y, _, _)| (y, x)));
        }
        let content: Vec<&Shape> = shapes.iter().collect();
        let side_by_side = content.len() == 2
            && (matches!(
                layout_type.as_str(),
                "twoObj" | "twoTxTwoObj" | "objTx" | "txObj" | "picTx"
            ) || match (content[0].pos, content[1].pos) {
                (Some((x0, y0, w0, h0)), Some((x1, y1, _w1, h1))) => {
                    x1 >= x0 + w0 * 8 / 10 && y1 < y0 + h0 && y0 < y1 + h1
                }
                _ => false,
            });
        let mut blocks = Vec::new();
        let mut body_paragraphs = Vec::new();
        if side_by_side {
            let mut columns = Vec::new();
            for s in &content {
                let b = self.shape_blocks(s, &mut body_paragraphs);
                columns.push(format!("::: column\n{}\n:::", b.join("\n\n")));
            }
            blocks.push(format!(":::: columns\n{}\n::::", columns.join("\n\n")));
        } else {
            for s in &content {
                blocks.extend(self.shape_blocks(s, &mut body_paragraphs));
            }
        }
        let mut notes = Vec::new();
        if let Some(np) = notes_part {
            if let Some(nroot) = self.pkg.xml(&np)? {
                let saved = std::mem::replace(&mut self.rels, self.pkg.rels(&np)?);
                let mut nshapes = Vec::new();
                if let Some(tree) = nroot.path(&["cSld", "spTree"]) {
                    collect(tree, &mut nshapes);
                }
                for s in nshapes
                    .iter()
                    .filter(|s| s.ph_type.as_deref() == Some("body"))
                {
                    // Notes paragraphs are prose: the notes master's bullets are not
                    // the author's.
                    let prose = Shape {
                        node: s.node,
                        ph_type: None,
                        is_placeholder: false,
                        pos: None,
                    };
                    let mut ignored = Vec::new();
                    notes.extend(self.shape_blocks(&prose, &mut ignored));
                }
                self.rels = saved;
            }
        }
        let cover = index == 0 && centred && blocks.is_empty();
        Ok(Slide {
            path: path.to_string(),
            title,
            section: layout_type == "secHead",
            cover,
            subtitle_lines,
            blocks,
            body_paragraphs,
            notes,
        })
    }

    /// A title's lines: its own emphasis (a bold title box) is the title's look, not
    /// the author's.
    fn title_lines(&mut self, sp: &Node) -> Vec<String> {
        self.text_lines(sp)
            .into_iter()
            .map(|l| {
                let unwrapped = l
                    .strip_prefix("**")
                    .and_then(|x| x.strip_suffix("**"))
                    .filter(|x| !x.contains("**"));
                unwrapped.map(str::to_string).unwrap_or(l)
            })
            .collect()
    }

    /// The plain lines of a shape's text (titles, subtitles).
    fn text_lines(&mut self, sp: &Node) -> Vec<String> {
        let Some(body) = sp.child("txBody") else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for p in body.children_named("p") {
            let (pieces, _) = self.runs(p);
            let text = md::inline(&pieces, false);
            for l in text.split("\\\n") {
                lines.push(l.trim().to_string());
            }
        }
        lines
    }

    fn shape_blocks(&mut self, shape: &Shape, plain_out: &mut Vec<String>) -> Vec<String> {
        let node = shape.node;
        match node.local() {
            "sp" => {
                let paras = self.paragraphs(node, shape.is_placeholder);
                plain_out.extend(paras.iter().map(|p| p.text.clone()));
                group(&paras)
            }
            "pic" => self.picture(node).into_iter().collect(),
            "graphicFrame" => {
                if let Some(tbl) = node.find("tbl") {
                    vec![self.table(tbl)]
                } else {
                    let uri = node
                        .find("graphicData")
                        .and_then(|g| g.attr("uri"))
                        .unwrap_or("")
                        .to_string();
                    let what = if uri.contains("chart") {
                        "a chart"
                    } else if uri.contains("diagram") {
                        "a SmartArt diagram"
                    } else {
                        "an embedded object"
                    };
                    self.warn_once(
                        "twin-unsupported-graphic",
                        what,
                        format!("{what} cannot be turned back into Markdown and was left out"),
                    );
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn paragraphs(&mut self, sp: &Node, placeholder: bool) -> Vec<Para> {
        let Some(body) = sp.child("txBody") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in body.children_named("p") {
            let ppr = p.child("pPr");
            let level = ppr
                .and_then(|x| x.attr("lvl"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0usize);
            let bullet = match ppr {
                Some(x) if x.child("buNone").is_some() => Bullet::None,
                Some(x) if x.child("buAutoNum").is_some() => Bullet::Ordered,
                Some(x) if x.child("buChar").is_some() || x.child("buBlip").is_some() => {
                    Bullet::Unordered
                }
                _ if placeholder => Bullet::Unordered,
                _ => Bullet::None,
            };
            let (pieces, mono) = self.runs(p);
            let text = if mono {
                code_text(&pieces)
            } else {
                md::inline(&pieces, false).trim().to_string()
            };
            if text.trim().is_empty() && !mono {
                continue;
            }
            out.push(Para {
                level,
                bullet,
                text,
                mono,
            });
        }
        out
    }

    /// The runs of a DrawingML paragraph, and whether every text run is monospace.
    fn runs(&mut self, p: &Node) -> (Vec<Piece>, bool) {
        let mut pieces = Vec::new();
        let mut text_runs = 0;
        let mut mono_runs = 0;
        for child in p.elems() {
            match child.local() {
                "r" | "fld" => {
                    let rpr = child.child("rPr");
                    let mut fmt = Fmt::default();
                    let mut mono = false;
                    if let Some(r) = rpr {
                        fmt.bold = matches!(r.attr("b"), Some("1") | Some("true"));
                        fmt.italic = matches!(r.attr("i"), Some("1") | Some("true"));
                        fmt.strike = r.attr("strike").is_some_and(|s| s != "noStrike");
                        match r.attr("baseline").and_then(|b| b.parse::<i64>().ok()) {
                            Some(b) if b > 0 => fmt.sup = true,
                            Some(b) if b < 0 => fmt.sub = true,
                            _ => {}
                        }
                        mono = r
                            .child("latin")
                            .and_then(|l| l.attr("typeface"))
                            .is_some_and(is_mono_name);
                    }
                    fmt.code = mono;
                    let text = child.child("t").map(Node::text).unwrap_or_default();
                    if text.is_empty() {
                        continue;
                    }
                    text_runs += 1;
                    if mono {
                        mono_runs += 1;
                    }
                    let link = rpr.and_then(|r| r.child("hlinkClick"));
                    match link {
                        Some(h)
                            if h.attr("action").is_some_and(|a| a.contains("hlinksldjump"))
                                && fmt.sup
                                && text.chars().all(|c| c.is_ascii_digit()) =>
                        {
                            // Pandoc's footnote marker: a superscript number jumping to
                            // the notes slide.
                            let target = h
                                .attr("id")
                                .and_then(|id| self.rels.get(id))
                                .map(|r| r.target.clone())
                                .unwrap_or_default();
                            self.note_refs.push((text.clone(), target));
                            pieces.push(Piece::Raw(format!("[^{text}]")));
                        }
                        Some(h) => {
                            let url = h
                                .attr("id")
                                .and_then(|id| self.rels.get(id))
                                .filter(|r| r.external)
                                .map(|r| r.target.clone());
                            match url {
                                Some(u) => {
                                    pieces.push(Piece::Link(u, vec![Piece::Text(text, fmt)]))
                                }
                                None => pieces.push(Piece::Text(text, fmt)),
                            }
                        }
                        None => pieces.push(Piece::Text(text, fmt)),
                    }
                }
                "br" => pieces.push(Piece::Break),
                // `a14:m`, inside the `mc:Choice` an equation's shape is written in: the
                // OMML back to LaTeX (the `mc:Fallback` copy is never read).
                "m" => {
                    for m in child.elems() {
                        let display = m.local() == "oMathPara";
                        let maths: Vec<&Node> = if display {
                            m.children_named("oMath").collect()
                        } else if m.local() == "oMath" {
                            vec![m]
                        } else {
                            Vec::new()
                        };
                        for one in maths {
                            let (tex, unknown) = super::math::to_latex(one);
                            for u in unknown {
                                self.warn_once(
                                    "twin-math-unknown",
                                    &u,
                                    format!("equation element `{u}` has no LaTeX equivalent here; its text was kept"),
                                );
                            }
                            text_runs += 1;
                            pieces.push(Piece::Raw(if display {
                                format!("$${tex}$$")
                            } else {
                                format!("${tex}$")
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        // Adjacent runs of one hyperlink merge into one link.
        let mut merged: Vec<Piece> = Vec::new();
        for piece in pieces {
            match (merged.last_mut(), piece) {
                (Some(Piece::Link(u0, inner)), Piece::Link(u1, more)) if *u0 == u1 => {
                    inner.extend(more)
                }
                (_, piece) => merged.push(piece),
            }
        }
        (merged, text_runs > 0 && mono_runs == text_runs)
    }

    fn picture(&mut self, pic: &Node) -> Option<String> {
        let rid = pic.find("blip")?.attr("embed")?.to_string();
        if !self.options.images {
            self.dropped_images += 1;
            return None;
        }
        let alt = pic
            .find("cNvPr")
            .and_then(|c| c.attr("descr").filter(|d| !d.is_empty()))
            .unwrap_or("")
            .to_string();
        let rel = self.rels.get(&rid).cloned()?;
        let src = if rel.external {
            rel.target
        } else {
            if let Some(p) = self.media_by_part.get(&rel.target) {
                p.clone()
            } else {
                let Some(bytes) = self.pkg.bytes(&rel.target) else {
                    self.warn_once(
                        "twin-missing-media",
                        &rel.target,
                        format!("image part `{}` is referenced but missing", rel.target),
                    );
                    return None;
                };
                let path = media_path(&self.options.media_dir, &rel.target, &mut self.used_names);
                self.media.push((path.clone(), bytes.to_vec()));
                self.media_by_part.insert(rel.target.clone(), path.clone());
                path
            }
        };
        Some(format!(
            "![{}]({})",
            md::escape(&alt, false, ""),
            md::link_target(&src)
        ))
    }

    fn table(&mut self, tbl: &Node) -> String {
        let mut rows = Vec::new();
        for tr in tbl.children_named("tr") {
            let mut row = Vec::new();
            for tc in tr.children_named("tc") {
                if tc.attr("hMerge").is_some() {
                    row.push(String::new());
                    continue;
                }
                let mut parts = Vec::new();
                if let Some(body) = tc.child("txBody") {
                    for p in body.children_named("p") {
                        let (pieces, _) = self.runs(p);
                        let s = md::inline(&pieces, true);
                        if !s.trim().is_empty() {
                            parts.push(s.trim().to_string());
                        }
                    }
                }
                row.push(parts.join(" "));
            }
            rows.push(row);
        }
        let aligns: Vec<Align> = Vec::new();
        md::table(&rows, &aligns)
    }
}

/// A text box that plays the title: named so, or one short line set at 32 pt or more.
fn looks_like_title_box(s: &Shape) -> bool {
    if !s.node.is("sp") || s.is_placeholder {
        return false;
    }
    let named = s
        .node
        .find("cNvPr")
        .and_then(|c| c.attr("name"))
        .is_some_and(|n| n.to_ascii_lowercase().starts_with("title"));
    let Some(body) = s.node.child("txBody") else {
        return false;
    };
    let paras: Vec<&Node> = body.children_named("p").collect();
    if paras.len() != 1 {
        return false;
    }
    let mut runs = Vec::new();
    paras[0].find_all("rPr", &mut runs);
    let large = !runs.is_empty()
        && runs.iter().all(|r| {
            r.attr("sz")
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|sz| sz >= 3200)
        });
    named || large
}

/// Flatten a shape tree (groups included) into drawables, in document order.
fn collect<'a>(tree: &'a Node, out: &mut Vec<Shape<'a>>) {
    for child in tree.elems() {
        match child.local() {
            "sp" | "pic" | "graphicFrame" => {
                let nv = child.elems().find(|n| n.local().starts_with("nv"));
                let ph = nv.and_then(|n| n.path(&["nvPr", "ph"]));
                let pos = child
                    .elems()
                    .find(|n| n.is("spPr") || n.is("xfrm"))
                    .and_then(|n| {
                        if n.is("xfrm") {
                            Some(n)
                        } else {
                            n.child("xfrm")
                        }
                    })
                    .and_then(|x| {
                        let off = x.child("off")?;
                        let ext = x.child("ext")?;
                        let num = |n: &Node, k: &str| n.attr(k).and_then(|v| v.parse::<i64>().ok());
                        Some((
                            num(off, "x")?,
                            num(off, "y")?,
                            num(ext, "cx")?,
                            num(ext, "cy")?,
                        ))
                    });
                out.push(Shape {
                    node: child,
                    ph_type: ph.map(|p| p.attr("type").unwrap_or("body").to_string()),
                    is_placeholder: ph.is_some(),
                    pos,
                });
            }
            "grpSp" => collect(child, out),
            "AlternateContent" => {
                if let Some(c) = child.child("Choice").or_else(|| child.child("Fallback")) {
                    collect(c, out);
                }
            }
            _ => {}
        }
    }
}

fn code_text(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|x| match x {
            Piece::Text(t, _) => t.clone(),
            Piece::Break => "\n".into(),
            Piece::Raw(r) => r.clone(),
            Piece::Link(_, inner) => code_text(inner),
        })
        .collect()
}

/// Group a body's paragraphs into Markdown blocks: lists, code blocks, paragraphs.
fn group(paras: &[Para]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < paras.len() {
        let p = &paras[i];
        if p.mono {
            let mut lines = Vec::new();
            while let Some(q) = paras.get(i).filter(|q| q.mono) {
                lines.push(q.text.clone());
                i += 1;
            }
            out.push(md::code_block("", &lines.join("\n")));
            continue;
        }
        if p.bullet == Bullet::None {
            out.push(md::guard_line_start(&p.text));
            i += 1;
            continue;
        }
        let mut lines = Vec::new();
        let mut columns: Vec<usize> = Vec::new();
        let mut counters: Vec<u64> = Vec::new();
        let kind = p.bullet;
        while let Some(q) = paras.get(i).filter(|q| {
            // A change of list kind at the top level starts another list.
            !q.mono && q.bullet != Bullet::None && (q.level > 0 || q.bullet == kind)
        }) {
            columns.truncate(q.level);
            counters.truncate(q.level + 1);
            let indent = columns.last().copied().unwrap_or(0);
            while columns.len() < q.level {
                columns.push(indent);
            }
            while counters.len() <= q.level {
                counters.push(0);
            }
            counters[q.level] += 1;
            let marker = if q.bullet == Bullet::Ordered {
                format!("{}. ", counters[q.level])
            } else {
                "- ".to_string()
            };
            let body = md::guard_line_start(&q.text);
            lines.push(format!(
                "{}{}{}",
                " ".repeat(indent),
                marker,
                md::indent(&body, indent + marker.len(), false)
            ));
            columns.push(indent + marker.len());
            i += 1;
        }
        out.push(lines.join("\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(level: usize, bullet: Bullet, text: &str) -> Para {
        Para {
            level,
            bullet,
            text: text.into(),
            mono: false,
        }
    }

    #[test]
    fn bullets_nest_and_number() {
        let md = group(&[
            para(0, Bullet::None, "Intro"),
            para(0, Bullet::Unordered, "a"),
            para(1, Bullet::Ordered, "b"),
            para(1, Bullet::Ordered, "c"),
            para(0, Bullet::Unordered, "d"),
        ]);
        assert_eq!(md, vec!["Intro", "- a\n  1. b\n  2. c\n- d"]);
    }

    #[test]
    fn notes_slide_numbers_are_stripped() {
        assert_eq!(
            numbered_notes(&["1\\. First.".into(), "2. Second.".into()]),
            vec!["First.".to_string(), "Second.".to_string()]
        );
    }
}
