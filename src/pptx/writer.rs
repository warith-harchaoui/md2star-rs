//! PresentationML out: slides built on the template's own layouts, packed into a zip.
//!
//! The template's package is kept whole — masters, layouts, theme, table styles,
//! properties — minus its example slides (and the media only they used). Each slide is
//! written against the layout its [`super::fit::Plan`] chose: the layout's placeholders
//! are instantiated by `type`/`idx` (no box written, so the slide inherits the layout's
//! geometry and styles), and only what the plan placed is emitted, so an unfilled
//! placeholder never appears as a "Click to add title" ghost. Tables are real `a:tbl`
//! frames, code a dark panel in a monospaced face, pictures `p:pic` contain-fitted into
//! their box with the alt text as their description, hyperlinks real relationships,
//! speaker notes a `notesSlide` (a notes master is added when the template has none).
//!
//! Determinism: parts are written in a fixed order with a fixed mtime, ids are
//! sequential, media are numbered by first use, and `docProps/core.xml` carries a fixed
//! timestamp — the same Markdown gives the same bytes on every run.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use super::content::{printed, Bullet, Para, Picture, TableUnit};
use super::fit::{self, Placed, Plan, Typo, EMU_PER_PT};
use super::fonts::Measure;
use super::layout::Placement;
use super::linebreak::break_offsets;
use super::template::{Package, Placeholder, TemplateInfo};
use super::xml::{self, esc, Node};
use crate::ast::{Align, Block, Inline, Warning};
use crate::error::{Error, Result};
use crate::resources::{ImageFormat, LoadedImage};

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const CT_NOTES: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const CT_NOTES_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
/// The fixed creation/modification time of every deck (byte idempotence).
pub const FIXED_TIMESTAMP: &str = "2001-01-01T00:00:00Z";
/// The name of a photo credit shape: the audit knows its small type is deliberate.
pub const CREDIT_NAME: &str = "md2star-credit";
/// The code panel's colours.
const CODE_BG: &str = "141416";
const CODE_FG: &str = "EAEAEA";
/// The 1×1 transparent PNG that stands in for an SVG where SVG is not understood.
const BLANK_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Everything the writer needs besides the placements.
pub struct DeckInput<'a> {
    /// The template package.
    pub template: &'a Package,
    /// What it offers.
    pub info: &'a TemplateInfo,
    /// Sizes and faces.
    pub typo: &'a Typo,
    /// The slides, placed.
    pub slides: &'a [Placement],
    /// Images by `src`.
    pub images: &'a BTreeMap<String, LoadedImage>,
    /// Footnote numbers by label.
    pub footnotes: &'a BTreeMap<String, usize>,
    /// The deck's language (`lang` attribute of every run).
    pub lang: String,
    /// `docProps/core.xml` title and author.
    pub title: String,
    /// The author(s).
    pub author: String,
}

// ── text runs ───────────────────────────────────────────────────────────────

/// One stretch of text with one style.
#[derive(Debug, Clone, Default, PartialEq)]
struct Seg {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
    baseline: i32,
    code: bool,
    link: Option<String>,
    /// An equation: its OMML, written as `a14:m` (Pandoc's form, which PowerPoint
    /// edits as a native equation); `text` keeps the LaTeX for measuring.
    math: Option<String>,
}

fn segs(inlines: &[Inline], style: &Seg, footnotes: &BTreeMap<String, usize>, out: &mut Vec<Seg>) {
    for inline in inlines {
        let mut s = style.clone();
        match inline {
            Inline::Text(t) => {
                s.text = t.clone();
                out.push(s);
            }
            Inline::Code(t) => {
                s.text = t.clone();
                s.code = true;
                out.push(s);
            }
            Inline::Math { tex, display } => {
                let omml = crate::docx::math::convert(tex, *display).xml;
                s.text = tex.clone();
                s.math = Some(declare_math_ns(&omml));
                out.push(s);
            }
            Inline::Emph(i) => {
                s.italic = true;
                segs(i, &s, footnotes, out);
            }
            Inline::Strong(i) => {
                s.bold = true;
                segs(i, &s, footnotes, out);
            }
            Inline::Strikeout(i) => {
                s.strike = true;
                segs(i, &s, footnotes, out);
            }
            Inline::Superscript(i) => {
                s.baseline = 30_000;
                segs(i, &s, footnotes, out);
            }
            Inline::Subscript(i) => {
                s.baseline = -25_000;
                segs(i, &s, footnotes, out);
            }
            Inline::Link { text, url, .. } => {
                if !url.starts_with('#') && !url.is_empty() {
                    s.link = Some(url.clone());
                }
                segs(text, &s, footnotes, out);
            }
            Inline::Image(img) => {
                s.text = printed(&img.alt);
                out.push(s);
            }
            Inline::Cite(c) => {
                s.text = crate::ast::cite_text(c);
                out.push(s);
            }
            Inline::FootnoteRef(label) => {
                s.text = footnotes
                    .get(label)
                    .map_or_else(|| label.clone(), |n| n.to_string());
                s.baseline = 30_000;
                out.push(s);
            }
            Inline::InlineNote(_) => {
                s.text = "*".into();
                s.baseline = 30_000;
                out.push(s);
            }
            Inline::SoftBreak => {
                s.text = " ".into();
                out.push(s);
            }
            Inline::HardBreak => {
                s.text = "\n".into();
                out.push(s);
            }
            Inline::RawHtml(_) => {}
        }
    }
}

/// How a paragraph's runs are written.
struct RunStyle<'a> {
    lang: &'a str,
    /// Explicit size (hundredths of a point), when it differs from the inherited one.
    sz: Option<u32>,
    bold: bool,
    italic: bool,
    color: Option<&'a str>,
    mono: &'a str,
}

/// The slide's relationships, built as runs and pictures need them.
#[derive(Default)]
struct Rels {
    entries: Vec<(String, String, String, bool)>,
}

impl Rels {
    fn add(&mut self, kind: &str, target: &str, external: bool) -> String {
        if let Some((id, ..)) = self
            .entries
            .iter()
            .find(|(_, k, t, e)| k == kind && t == target && *e == external)
        {
            return id.clone();
        }
        let id = format!("rId{}", self.entries.len() + 1);
        self.entries
            .push((id.clone(), kind.to_string(), target.to_string(), external));
        id
    }

    fn xml(&self) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        );
        for (id, kind, target, external) in &self.entries {
            s.push_str(&format!(
                "<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{}\"{}/>",
                esc(target),
                if *external {
                    " TargetMode=\"External\""
                } else {
                    ""
                }
            ));
        }
        s.push_str("</Relationships>");
        s
    }
}

fn rpr(seg: &Seg, st: &RunStyle, rels: &mut Rels) -> String {
    let mut a = format!("<a:rPr lang=\"{}\"", esc(st.lang));
    if let Some(sz) = st.sz {
        a.push_str(&format!(" sz=\"{sz}\""));
    }
    if seg.bold || st.bold {
        a.push_str(" b=\"1\"");
    }
    if seg.italic || st.italic {
        a.push_str(" i=\"1\"");
    }
    if seg.strike {
        a.push_str(" strike=\"sngStrike\"");
    }
    if seg.baseline != 0 {
        a.push_str(&format!(" baseline=\"{}\"", seg.baseline));
    }
    a.push_str(" dirty=\"0\">");
    if let Some(c) = st.color {
        a.push_str(&format!(
            "<a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill>"
        ));
    }
    if seg.code {
        let m = esc(st.mono);
        a.push_str(&format!(
            "<a:latin typeface=\"{m}\"/><a:cs typeface=\"{m}\"/>"
        ));
    }
    if let Some(url) = &seg.link {
        let id = rels.add("hyperlink", url, true);
        a.push_str(&format!("<a:hlinkClick r:id=\"{id}\"/>"));
    }
    a.push_str("</a:rPr>");
    a
}

/// The runs of a paragraph, with line breaks at `breaks` (byte offsets into the
/// concatenated text where a new line starts; the spaces before are dropped).
fn runs_xml(segments: &[Seg], breaks: &[usize], st: &RunStyle, rels: &mut Rels) -> String {
    let full: String = segments.iter().map(|s| s.text.as_str()).collect();
    // Byte ranges to drop: the spaces before each chosen break.
    let mut drop: Vec<(usize, usize)> = Vec::new();
    for &b in breaks {
        let start = full[..b].trim_end_matches(' ').len();
        drop.push((start, b));
    }
    let mut out = String::new();
    let mut offset = 0usize;
    for seg in segments {
        if let Some(omml) = &seg.math {
            offset += seg.text.len();
            // Sentinels [with_math] resolves: the equation for PowerPoint, its LaTeX as
            // an italic run for readers without the extension (LibreOffice).
            // Each math run carries the paragraph's size, as PowerPoint writes it.
            let omml = omml.replace(
                "<m:t>",
                &format!(
                    "<a:rPr lang=\"{}\"{} dirty=\"0\"/><m:t>",
                    esc(st.lang),
                    st.sz.map(|z| format!(" sz=\"{z}\"")).unwrap_or_default()
                ),
            );
            out.push_str(&format!(
                "\u{1}{omml}\u{2}<a:r>{}<a:t>{}</a:t></a:r>\u{3}",
                rpr(
                    &Seg {
                        italic: true,
                        ..seg.clone()
                    },
                    st,
                    rels
                ),
                esc(&seg.text)
            ));
            continue;
        }
        let mut buf = String::new();
        let flush = |buf: &mut String, out: &mut String, rels: &mut Rels| {
            if !buf.is_empty() {
                out.push_str("<a:r>");
                out.push_str(&rpr(seg, st, rels));
                out.push_str(&format!("<a:t>{}</a:t></a:r>", esc(buf)));
                buf.clear();
            }
        };
        for ch in seg.text.chars() {
            let here = offset;
            offset += ch.len_utf8();
            if drop.iter().any(|&(s, e)| here >= s && here < e) {
                continue;
            }
            if breaks.contains(&here) || ch == '\n' {
                flush(&mut buf, &mut out, rels);
                out.push_str(&format!(
                    "<a:br><a:rPr lang=\"{}\"{} dirty=\"0\"/></a:br>",
                    esc(st.lang),
                    st.sz.map(|s| format!(" sz=\"{s}\"")).unwrap_or_default()
                ));
                if ch == '\n' {
                    continue;
                }
            }
            buf.push(ch);
        }
        flush(&mut buf, &mut out, rels);
    }
    out
}

/// Where a paragraph sits and how it is broken.
struct ParaCtx<'a> {
    style: RunStyle<'a>,
    /// Line-breaking: (size pt, width pt, measure); `None` leaves wrapping to the renderer.
    breaking: Option<(f32, f32, Measure)>,
    /// In a placeholder (bullets inherited) or a free box (bullets explicit).
    placeholder: bool,
    center: bool,
    /// Points the text starts in from the box's inner edge (for line breaking).
    indent: f32,
    /// A placeholder bullet's margins scaled with its raised size.
    margins: Option<(i64, i64)>,
}

fn para_xml(
    p: &Para,
    pc: &ParaCtx,
    footnotes: &BTreeMap<String, usize>,
    rels: &mut Rels,
) -> String {
    let mut segments = Vec::new();
    if let Bullet::Task(done) = p.bullet {
        segments.push(Seg {
            text: if done { "☑ " } else { "☐ " }.into(),
            ..Seg::default()
        });
    }
    let base = Seg {
        bold: p.bold,
        italic: p.italic,
        ..Seg::default()
    };
    segs(&p.inlines, &base, footnotes, &mut segments);
    let full: String = segments.iter().map(|s| s.text.as_str()).collect();
    let mut breaks = Vec::new();
    if let Some((size, width, measure)) = &pc.breaking {
        let w = fit::para_width(*width, pc.indent);
        let mut base_off = 0;
        for line in full.split('\n') {
            breaks.extend(
                break_offsets(line, *size, w, measure)
                    .into_iter()
                    .map(|o| o + base_off),
            );
            base_off += line.len() + 1;
        }
    }
    let mut ppr = String::from("<a:pPr");
    let indent_emu = 342_900i64;
    match (pc.placeholder, p.bullet) {
        (true, Bullet::None | Bullet::Task(_)) => {
            ppr.push_str(&format!(
                " marL=\"{}\" indent=\"0\"",
                457_200 * p.level as i64
            ));
        }
        (true, _) => {
            if p.level > 0 {
                ppr.push_str(&format!(" lvl=\"{}\"", p.level.min(8)));
            }
            if let Some((m, i)) = pc.margins {
                ppr.push_str(&format!(" marL=\"{m}\" indent=\"{i}\""));
            }
        }
        (false, Bullet::None | Bullet::Task(_)) => {
            ppr.push_str(&format!(
                " marL=\"{}\" indent=\"0\"",
                457_200 * p.level as i64
            ));
        }
        (false, _) => {
            ppr.push_str(&format!(
                " marL=\"{}\" indent=\"-{indent_emu}\"",
                indent_emu * (p.level as i64 + 1)
            ));
        }
    }
    if pc.center {
        ppr.push_str(" algn=\"ctr\"");
    }
    ppr.push('>');
    match p.bullet {
        Bullet::None | Bullet::Task(_) => ppr.push_str("<a:buNone/>"),
        Bullet::Number(n) => ppr.push_str(&format!(
            "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\" startAt=\"{n}\"/>"
        )),
        Bullet::Char if !pc.placeholder => {
            ppr.push_str("<a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/>")
        }
        Bullet::Char => {}
    }
    ppr.push_str("</a:pPr>");
    let st = &pc.style;
    let end = format!(
        "<a:endParaRPr lang=\"{}\"{} dirty=\"0\"/>",
        esc(st.lang),
        st.sz.map(|s| format!(" sz=\"{s}\"")).unwrap_or_default()
    );
    format!(
        "<a:p>{ppr}{}{end}</a:p>",
        runs_xml(&segments, &breaks, st, rels)
    )
}

// ── shapes ──────────────────────────────────────────────────────────────────

/// The OMML fragment with `xmlns:m` declared on its root element (`m:oMath` or
/// `m:oMathPara`), so it stands alone inside `a14:m`.
fn declare_math_ns(omml: &str) -> String {
    let Some(start) = omml.find("<m:") else {
        return omml.to_string();
    };
    let name_end = omml[start + 1..]
        .find([' ', '>', '/'])
        .map_or(omml.len(), |i| start + 1 + i);
    format!(
        "{} xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"{}",
        &omml[..name_end],
        &omml[name_end..]
    )
}

/// A shape holding an equation goes inside `mc:AlternateContent`, as Pandoc writes it
/// (`a14:m` belongs to the Office 2010 drawing extensions), with a fallback where each
/// equation is its LaTeX in italics, for readers without the extension.
fn with_math(shape: String) -> String {
    if !shape.contains('\u{1}') {
        return shape;
    }
    let (mut choice, mut fallback) = (String::new(), String::new());
    let mut rest = shape.as_str();
    while let Some(a) = rest.find('\u{1}') {
        let b = rest[a..].find('\u{2}').map_or(rest.len(), |i| a + i);
        let c = rest[b..].find('\u{3}').map_or(rest.len(), |i| b + i);
        choice.push_str(&rest[..a]);
        fallback.push_str(&rest[..a]);
        choice.push_str(&format!("<a14:m>{}</a14:m>", &rest[a + 1..b]));
        fallback.push_str(&rest[(b + 1).min(c)..c]);
        rest = &rest[(c + 1).min(rest.len())..];
    }
    choice.push_str(rest);
    fallback.push_str(rest);
    format!(
        "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" Requires=\"a14\">{choice}</mc:Choice><mc:Fallback>{fallback}</mc:Fallback></mc:AlternateContent>"
    )
}

fn xfrm(f: (i64, i64, i64, i64)) -> String {
    format!(
        "<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
        f.0,
        f.1,
        f.2.max(1),
        f.3.max(1)
    )
}

fn ph_tag(ph: &Placeholder) -> String {
    let mut s = String::from("<p:ph");
    if ph.kind != "body" || ph.idx == 0 {
        s.push_str(&format!(" type=\"{}\"", ph.kind));
    }
    if ph.idx != 0 {
        s.push_str(&format!(" idx=\"{}\"", ph.idx));
    }
    s.push_str("/>");
    s
}

struct SlideWriter<'a> {
    input: &'a DeckInput<'a>,
    rels: Rels,
    shapes: String,
    next_id: u32,
    media: &'a mut Media,
    no_autofit: bool,
}

impl SlideWriter<'_> {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn placeholder_text(
        &mut self,
        ph: &Placeholder,
        frame: Option<(i64, i64, i64, i64)>,
        paras: &[Para],
        title: bool,
    ) {
        let id = self.id();
        let name = if ph.name.is_empty() {
            format!("Placeholder {}", id - 1)
        } else {
            ph.name.clone()
        };
        let sppr = match frame {
            Some(f) => format!("<p:spPr>{}</p:spPr>", xfrm(f)),
            None => "<p:spPr/>".into(),
        };
        let geometry = frame.or(ph.frame);
        let body_pr = if self.no_autofit {
            "<a:bodyPr><a:noAutofit/></a:bodyPr>"
        } else {
            "<a:bodyPr/>"
        };
        let mut body = String::new();
        let typo = self.input.typo;
        for p in paras {
            let declared = ph.size_pt(p.level);
            let eff = typo.eff(declared);
            let sz = ((eff - declared).abs() > 0.01).then(|| (eff * 100.0).round() as u32);
            let breaking = geometry.map(|g| {
                (
                    eff,
                    fit::usable_width_pt(g, ph.insets),
                    fit::measure_of(ph, p.bold),
                )
            });
            let pc = ParaCtx {
                style: RunStyle {
                    lang: &self.input.lang,
                    sz,
                    bold: false,
                    italic: false,
                    color: None,
                    mono: &typo.mono_font,
                },
                breaking,
                placeholder: true,
                center: false,
                indent: fit::indent_pt(p, Some(ph), typo),
                margins: (sz.is_some() && p.bullet != Bullet::None && !title)
                    .then(|| fit::scaled_margin(ph, p.level, typo)),
            };
            if title {
                // A title is one paragraph: no list marker of any kind.
                let mut p = p.clone();
                p.bullet = Bullet::Char;
                p.level = 0;
                body.push_str(&para_xml(&p, &pc, self.input.footnotes, &mut self.rels));
            } else {
                body.push_str(&para_xml(p, &pc, self.input.footnotes, &mut self.rels));
            }
        }
        self.shapes.push_str(&with_math(format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr>{}</p:nvPr></p:nvSpPr>{sppr}<p:txBody>{body_pr}<a:lstStyle/>{body}</p:txBody></p:sp>",
            esc(&name),
            ph_tag(ph)
        )));
    }

    fn free_text(
        &mut self,
        frame: (i64, i64, i64, i64),
        paras: &[Para],
        levels: &[f32; 9],
        center: bool,
        name: Option<&str>,
    ) {
        let id = self.id();
        let name = name.map_or_else(|| format!("TextBox {}", id - 1), str::to_string);
        let typo = self.input.typo;
        let measure = Measure::for_family(self.input.info.theme_fonts.1.as_deref(), false, false);
        let mut body = String::new();
        for p in paras {
            let size = levels[p.level.min(8)];
            let pc = ParaCtx {
                style: RunStyle {
                    lang: &self.input.lang,
                    sz: Some((size * 100.0).round() as u32),
                    bold: false,
                    italic: false,
                    color: None,
                    mono: &typo.mono_font,
                },
                breaking: Some((
                    size,
                    (frame.2 - 182_880) as f32 / EMU_PER_PT,
                    measure.clone(),
                )),
                placeholder: false,
                center,
                indent: fit::indent_pt(p, None, typo),
                margins: None,
            };
            body.push_str(&para_xml(p, &pc, self.input.footnotes, &mut self.rels));
        }
        let anchor = if center { "ctr" } else { "t" };
        self.shapes.push_str(&with_math(format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" anchor=\"{anchor}\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>",
            esc(&name),
            xfrm(frame)
        )));
    }

    fn code(&mut self, frame: (i64, i64, i64, i64), lines: &[String]) {
        let id = self.id();
        let typo = self.input.typo;
        let size = typo.code_pt();
        let sz = (size * 100.0).round() as u32;
        let mono = esc(&typo.mono_font);
        let lang = esc(&self.input.lang);
        let mut body = String::new();
        for line in lines {
            body.push_str("<a:p><a:pPr marL=\"0\" indent=\"0\" algn=\"l\"><a:buNone/></a:pPr>");
            if !line.is_empty() {
                body.push_str(&format!(
                    "<a:r><a:rPr lang=\"{lang}\" sz=\"{sz}\" dirty=\"0\"><a:solidFill><a:srgbClr val=\"{CODE_FG}\"/></a:solidFill><a:latin typeface=\"{mono}\"/><a:cs typeface=\"{mono}\"/></a:rPr><a:t>{}</a:t></a:r>",
                    esc(line)
                ));
            }
            body.push_str(&format!(
                "<a:endParaRPr lang=\"{lang}\" sz=\"{sz}\" dirty=\"0\"/></a:p>"
            ));
        }
        self.shapes.push_str(&format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Code {}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 3000\"/></a:avLst></a:prstGeom><a:solidFill><a:srgbClr val=\"{CODE_BG}\"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"{}\" tIns=\"{}\" rIns=\"{}\" bIns=\"{}\" anchor=\"t\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>",
            id - 1,
            xfrm(frame),
            fit::CODE_INSETS.0,
            fit::CODE_INSETS.1,
            fit::CODE_INSETS.0,
            fit::CODE_INSETS.1
        ));
    }

    fn table(&mut self, frame: (i64, i64, i64, i64), t: &TableUnit, size: f32) {
        let id = self.id();
        let measure = Measure::for_family(self.input.info.theme_fonts.1.as_deref(), false, false);
        let widths = fit::column_widths(t, frame.2);
        let heights = fit::row_heights(t, frame.2, size, &measure);
        let total_h: i64 = heights.iter().sum();
        let sz = (size * 100.0).round() as u32;
        let mut grid = String::new();
        for w in &widths {
            grid.push_str(&format!("<a:gridCol w=\"{w}\"/>"));
        }
        let mut rows = String::new();
        let cols = widths.len();
        for (r, (row, h)) in std::iter::once(&t.headers)
            .chain(t.rows.iter())
            .zip(&heights)
            .enumerate()
        {
            rows.push_str(&format!("<a:tr h=\"{h}\">"));
            for c in 0..cols {
                let empty = Vec::new();
                let cell = row.get(c).unwrap_or(&empty);
                let algn = match t.aligns.get(c) {
                    Some(Align::Center) => " algn=\"ctr\"",
                    Some(Align::Right) => " algn=\"r\"",
                    _ => "",
                };
                let mut segments = Vec::new();
                let base = Seg {
                    bold: r == 0,
                    ..Seg::default()
                };
                segs(cell, &base, self.input.footnotes, &mut segments);
                let st = RunStyle {
                    lang: &self.input.lang,
                    sz: Some(sz),
                    bold: false,
                    italic: false,
                    color: None,
                    mono: &self.input.typo.mono_font,
                };
                let runs = runs_xml(&segments, &[], &st, &mut self.rels);
                rows.push_str(&format!(
                    "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr{algn}/>{runs}<a:endParaRPr lang=\"{}\" sz=\"{sz}\" dirty=\"0\"/></a:p></a:txBody><a:tcPr/></a:tc>",
                    esc(&self.input.lang)
                ));
            }
            rows.push_str("</a:tr>");
        }
        let style = table_style(self.input.template);
        self.shapes.push_str(&format!(
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"1\" bandRow=\"1\"><a:tableStyleId>{style}</a:tableStyleId></a:tblPr><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
            id - 1,
            frame.0,
            frame.1,
            frame.2,
            total_h
        ));
    }

    fn picture(&mut self, frame: (i64, i64, i64, i64), pic: &Picture) {
        let Some(img) = self.input.images.get(&pic.key) else {
            return;
        };
        let id = self.id();
        let credit = pic.image.title.as_deref().map(short_credit);
        let link = credit.as_ref().and_then(|(_, url)| url.clone());
        let hlink = link
            .as_ref()
            .map(|u| {
                format!(
                    "<a:hlinkClick r:id=\"{}\"/>",
                    self.rels.add("hyperlink", u, true)
                )
            })
            .unwrap_or_default();
        let descr = pic.image.alt_text();
        let blip = if img.info.format == ImageFormat::Svg {
            let svg = self.media.add(&img.bytes, "svg");
            let fallback =
                super::images::svg_fallback_png(&img.bytes, img.info.width_px, img.info.height_px)
                    .unwrap_or_else(|| BLANK_PNG.to_vec());
            let png = self.media.add(&fallback, "png");
            let png_id = self.rels.add("image", &format!("../media/{png}"), false);
            let svg_id = self.rels.add("image", &format!("../media/{svg}"), false);
            super::images::svg_blip(&png_id, &svg_id)
        } else {
            let name = self.media.add(&img.bytes, img.info.format.extension());
            let rid = self.rels.add("image", &format!("../media/{name}"), false);
            super::images::raster_blip(&rid)
        };
        let frame = super::images::fit_in(&img.info, &pic.image.attr, frame);
        self.shapes
            .push_str(&super::images::pic_xml(id, &descr, &blip, &hlink, frame));
        if let Some((text, url)) = credit {
            if !text.is_empty() {
                self.credit(frame, &text, url.as_deref());
            }
        }
    }

    /// A photo credit, small, white on a translucent dark plate, in the picture's
    /// bottom-right corner: 10 pt, or 9 pt on two lines when the picture is narrow.
    fn credit(&mut self, pic: (i64, i64, i64, i64), text: &str, url: Option<&str>) {
        let measure = Measure::for_family(self.input.info.theme_fonts.1.as_deref(), false, false);
        let margin = 91_440i64;
        let pad_x = 64_008i64;
        let pad_y = 27_432i64;
        let max_w = (pic.2 - 2 * margin - 2 * pad_x).max(1) as f32 / EMU_PER_PT;
        let (size, lines) = [10.0f32, 9.0]
            .iter()
            .map(|&s| (s, super::linebreak::wrap(text, s, max_w, &measure)))
            .find(|(_, l)| l.len() <= 1)
            .unwrap_or_else(|| (9.0, super::linebreak::wrap(text, 9.0, max_w, &measure)));
        let widest = lines
            .iter()
            .map(|l| measure.width(l, size))
            .fold(0.0f32, f32::max);
        let w = ((widest * EMU_PER_PT) as i64 + 2 * pad_x).min(pic.2 - 2 * margin);
        let h = (lines.len() as f32 * size * fit::LINE_SPACING * EMU_PER_PT) as i64 + 2 * pad_y;
        let frame = (pic.0 + pic.2 - margin - w, pic.1 + pic.3 - margin - h, w, h);
        let id = self.id();
        let link = url
            .map(|u| {
                format!(
                    "<a:hlinkClick r:id=\"{}\"/>",
                    self.rels.add("hyperlink", u, true)
                )
            })
            .unwrap_or_default();
        let sz = (size * 100.0).round() as u32;
        let lang = esc(&self.input.lang);
        let mut body = String::new();
        for line in &lines {
            body.push_str(&format!(
                "<a:p><a:pPr marL=\"0\" indent=\"0\" algn=\"r\"><a:buNone/></a:pPr><a:r><a:rPr lang=\"{lang}\" sz=\"{sz}\" dirty=\"0\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill>{link}</a:rPr><a:t>{}</a:t></a:r></a:p>",
                esc(line)
            ));
        }
        self.shapes.push_str(&format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{CREDIT_NAME}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"000000\"><a:alpha val=\"50000\"/></a:srgbClr></a:solidFill></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"{pad_x}\" tIns=\"{pad_y}\" rIns=\"{pad_x}\" bIns=\"{pad_y}\" anchor=\"ctr\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>",
            xfrm(frame)
        ));
    }
}

/// The words a credit prints and the full address it links to: `Courtesy of
/// https://www.nga.gov/artworks/195605` prints `Courtesy of nga.gov` and links to the
/// whole address (md2star 3.12's `short_credit`).
pub fn short_credit(text: &str) -> (String, Option<String>) {
    let Some(start) = text.find("http://").or_else(|| text.find("https://")) else {
        return (text.trim().to_string(), None);
    };
    let rest = &text[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ';', ')']);
    let after_scheme = url.split_once("://").map_or(url, |(_, r)| r);
    let netloc = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let host = netloc.rsplit('@').next().unwrap_or(netloc);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let shown = format!("{}{}{}", &text[..start], host, &text[start + url.len()..]);
    (shown.trim().to_string(), Some(url.to_string()))
}

fn table_style(template: &Package) -> String {
    template
        .xml("ppt/tableStyles.xml")
        .ok()
        .and_then(|n| n.attr("def").map(str::to_string))
        .filter(|s| s.starts_with('{'))
        .unwrap_or_else(|| "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}".into())
}

/// Media parts, numbered by first use, deduplicated by content.
#[derive(Default)]
struct Media {
    parts: Vec<(String, Vec<u8>)>,
}

impl Media {
    fn add(&mut self, bytes: &[u8], ext: &str) -> String {
        if let Some((name, _)) = self.parts.iter().find(|(_, b)| b.as_slice() == bytes) {
            return name.clone();
        }
        let name = format!("md2star-image{}.{ext}", self.parts.len() + 1);
        self.parts.push((name.clone(), bytes.to_vec()));
        name
    }
}

fn spt_head() -> &'static str {
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
}

fn notes_paras(
    blocks: &[Block],
    lang: &str,
    footnotes: &BTreeMap<String, usize>,
    rels: &mut Rels,
) -> String {
    let store = BTreeMap::new();
    let units = super::content::units(blocks, &store);
    let mut out = String::new();
    for u in units {
        let p = match u {
            super::content::Unit::Para(p) => p,
            super::content::Unit::Code(lines) => Para::plain(vec![Inline::Text(lines.join("\n"))]),
            super::content::Unit::Table(t) => Para::plain(vec![Inline::Text(
                std::iter::once(&t.headers)
                    .chain(t.rows.iter())
                    .map(|r| r.iter().map(|c| printed(c)).collect::<Vec<_>>().join(" | "))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )]),
            super::content::Unit::Picture(p) => Para::plain(p.image.alt.clone()),
        };
        let pc = ParaCtx {
            style: RunStyle {
                lang,
                sz: None,
                bold: false,
                italic: false,
                color: None,
                mono: "Consolas",
            },
            breaking: None,
            placeholder: false,
            center: false,
            indent: 0.0,
            margins: None,
        };
        let mut p = p;
        if p.bullet == Bullet::Char {
            p.inlines.insert(0, Inline::Text("• ".into()));
        }
        p.bullet = Bullet::None;
        out.push_str(&para_xml(&p, &pc, footnotes, rels));
    }
    if out.is_empty() {
        out.push_str(&format!(
            "<a:p><a:endParaRPr lang=\"{}\"/></a:p>",
            esc(lang)
        ));
    }
    out
}

const NOTES_MASTER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:notesMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"685800\" y=\"1143000\"/><a:ext cx=\"5486400\" cy=\"3086100\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln w=\"12700\"><a:solidFill><a:prstClr val=\"black\"/></a:solidFill></a:ln></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" sz=\"quarter\" idx=\"3\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"685800\" y=\"4400550\"/><a:ext cx=\"5486400\" cy=\"3600450\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr vert=\"horz\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" rtlCol=\"0\"/><a:lstStyle/><a:p><a:pPr lvl=\"0\"/><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:notesStyle><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:defRPr sz=\"1200\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:notesStyle></p:notesMaster>";

// ── the package ─────────────────────────────────────────────────────────────

fn serialize(node: &Node, out: &mut String) {
    out.push('<');
    out.push_str(&node.name);
    for (k, v) in &node.attrs {
        out.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    if node.children.is_empty() && node.text.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    out.push_str(&esc(&node.text));
    for c in &node.children {
        serialize(c, out);
    }
    out.push_str(&format!("</{}>", node.name));
}

fn to_xml(node: &Node) -> Vec<u8> {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    serialize(node, &mut s);
    s.into_bytes()
}

fn rel_node(id: &str, kind: &str, target: &str) -> Node {
    Node {
        name: "Relationship".into(),
        attrs: vec![
            ("Id".into(), id.into()),
            ("Type".into(), format!("{REL}/{kind}")),
            ("Target".into(), target.into()),
        ],
        ..Node::default()
    }
}

fn next_rid(rels: &Node) -> usize {
    rels.children
        .iter()
        .filter_map(|r| r.attr("Id")?.strip_prefix("rId")?.parse::<usize>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// Build the deck.
pub fn write(input: &DeckInput, warnings: &mut Vec<Warning>) -> Result<Vec<u8>> {
    let mut parts = input.template.parts.clone();
    let template = input.template;
    // 1. Drop the template's example slides, their notes, and media only they used.
    parts.retain(|name, _| {
        !(name.starts_with("ppt/slides/")
            || name.starts_with("ppt/notesSlides/")
            || name == "docProps/thumbnail.jpeg")
    });
    let mut referenced = std::collections::BTreeSet::new();
    for name in parts.keys() {
        if name.ends_with(".rels") {
            let (dir, file) = name
                .rsplit_once("/_rels/")
                .map_or(("", name.as_str()), |(d, f)| (d, f));
            let source = if dir.is_empty() {
                file.trim_end_matches(".rels").to_string()
            } else {
                format!("{dir}/{}", file.trim_end_matches(".rels"))
            };
            for r in template.rels(&source) {
                if !r.external {
                    referenced.insert(r.target);
                }
            }
        }
    }
    parts.retain(|name, _| !name.starts_with("ppt/media/") || referenced.contains(name));

    // 2. Slides.
    let mut media = Media::default();
    let mut slide_parts: Vec<(String, Vec<u8>, String, Vec<u8>)> = Vec::new();
    let mut notes_parts: Vec<(usize, Vec<u8>, Vec<u8>)> = Vec::new();
    let existing_notes_master = template
        .rels("ppt/presentation.xml")
        .into_iter()
        .find(|r| r.kind.ends_with("/notesMaster"))
        .map(|r| r.target);
    let notes_master = existing_notes_master
        .clone()
        .unwrap_or_else(|| "ppt/notesMasters/notesMaster1.xml".into());
    for (k, placement) in input.slides.iter().enumerate() {
        let n = k + 1;
        let plan: &Plan = &placement.plan;
        let layout = &input.info.layouts[plan.layout];
        let mut w = SlideWriter {
            input,
            rels: Rels::default(),
            shapes: String::new(),
            next_id: 1,
            media: &mut media,
            no_autofit: input.typo.min_pt.is_some(),
        };
        let layout_target = format!(
            "../slideLayouts/{}",
            layout.part.rsplit('/').next().unwrap_or("")
        );
        w.rels.add("slideLayout", &layout_target, false);
        if let Some(t) = plan.title_ph {
            if !placement.title.is_empty() {
                let ph = &layout.placeholders[t];
                let title = Para {
                    inlines: placement.title.clone(),
                    ..Para::plain(Vec::new())
                };
                w.placeholder_text(ph, plan.title_frame, &[title], true);
            }
        }
        for placed in &plan.placed {
            match placed {
                Placed::Text { ph, frame, paras } => {
                    let ph = &layout.placeholders[*ph];
                    w.placeholder_text(ph, *frame, paras, false);
                }
                Placed::Free {
                    frame,
                    paras,
                    levels,
                    center,
                    ..
                } => w.free_text(*frame, paras, levels, *center, None),
                Placed::Table { frame, table, size } => w.table(*frame, table, *size),
                Placed::Code { frame, lines } => w.code(*frame, lines),
                Placed::Picture { frame, pic } => w.picture(*frame, pic),
            }
        }
        if !placement.notes.is_empty() {
            let mut nrels = Rels::default();
            nrels.add(
                "notesMaster",
                &format!(
                    "../notesMasters/{}",
                    notes_master.rsplit('/').next().unwrap_or("")
                ),
                false,
            );
            nrels.add("slide", &format!("../slides/slide{n}.xml"), false);
            let mut link_rels = Rels::default();
            let body = notes_paras(
                &placement.notes,
                &input.lang,
                input.footnotes,
                &mut link_rels,
            );
            // Hyperlinks inside notes: renumber after the two fixed relationships.
            let mut body = body;
            for (id, kind, target, external) in link_rels.entries.iter().rev() {
                let new_id = nrels.add(kind, target, *external);
                body = body.replace(&format!("r:id=\"{id}\""), &format!("r:id=\"{new_id}\""));
            }
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:notes xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"><p:cSld><p:spTree>{}<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>",
                spt_head()
            );
            w.rels.add(
                "notesSlide",
                &format!("../notesSlides/notesSlide{n}.xml"),
                false,
            );
            notes_parts.push((n, xml.into_bytes(), nrels.xml().into_bytes()));
        }
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sld xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"><p:cSld><p:spTree>{}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            spt_head(),
            w.shapes
        );
        let rels_xml = w.rels.xml();
        slide_parts.push((
            format!("ppt/slides/slide{n}.xml"),
            xml.into_bytes(),
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            rels_xml.into_bytes(),
        ));
    }
    if input.slides.is_empty() {
        warnings.push(Warning::new("empty-deck", "the document has no slide"));
    }

    // 3. Notes master, when notes exist and the template has none.
    let mut new_overrides: Vec<(String, &str)> = Vec::new();
    let mut add_notes_master = false;
    if !notes_parts.is_empty() && existing_notes_master.is_none() {
        add_notes_master = true;
        let theme_n = (1..)
            .find(|i| !parts.contains_key(&format!("ppt/theme/theme{i}.xml")))
            .unwrap_or(99);
        let theme_src = template
            .rels("ppt/slideMasters/slideMaster1.xml")
            .into_iter()
            .find(|r| r.kind.ends_with("/theme"))
            .map(|r| r.target)
            .unwrap_or_else(|| "ppt/theme/theme1.xml".into());
        let theme = parts
            .get(&theme_src)
            .cloned()
            .ok_or_else(|| Error::Pptx("the template has no theme".into()))?;
        let theme_part = format!("ppt/theme/theme{theme_n}.xml");
        parts.insert(theme_part.clone(), theme);
        if let Some(rels) = parts
            .get(&format!(
                "ppt/theme/_rels/{}.rels",
                theme_src.rsplit('/').next().unwrap_or("")
            ))
            .cloned()
        {
            parts.insert(format!("ppt/theme/_rels/theme{theme_n}.xml.rels"), rels);
        }
        parts.insert(notes_master.clone(), NOTES_MASTER.as_bytes().to_vec());
        let mut r = Rels::default();
        r.add("theme", &format!("../theme/theme{theme_n}.xml"), false);
        parts.insert(
            "ppt/notesMasters/_rels/notesMaster1.xml.rels".into(),
            r.xml().into_bytes(),
        );
        new_overrides.push((format!("/{notes_master}"), CT_NOTES_MASTER));
        new_overrides.push((format!("/{theme_part}"), CT_THEME));
    }
    for (name, xml, rels_name, rels) in &slide_parts {
        parts.insert(name.clone(), xml.clone());
        parts.insert(rels_name.clone(), rels.clone());
        new_overrides.push((format!("/{name}"), CT_SLIDE));
    }
    for (n, xml, rels) in &notes_parts {
        parts.insert(format!("ppt/notesSlides/notesSlide{n}.xml"), xml.clone());
        parts.insert(
            format!("ppt/notesSlides/_rels/notesSlide{n}.xml.rels"),
            rels.clone(),
        );
        new_overrides.push((format!("/ppt/notesSlides/notesSlide{n}.xml"), CT_NOTES));
    }
    for (name, bytes) in &media.parts {
        parts.insert(format!("ppt/media/{name}"), bytes.clone());
    }

    // 4. presentation.xml and its relationships.
    let mut pres_rels = xml::parse(
        parts
            .get("ppt/_rels/presentation.xml.rels")
            .ok_or_else(|| Error::Pptx("the template has no presentation relationships".into()))?,
    )?;
    pres_rels.children.retain(|r| {
        let t = r.attr("Type").unwrap_or_default();
        !t.ends_with("/slide")
    });
    let mut rid = next_rid(&pres_rels);
    let mut slide_rids = Vec::new();
    for (name, ..) in &slide_parts {
        let id = format!("rId{rid}");
        rid += 1;
        pres_rels
            .children
            .push(rel_node(&id, "slide", name.trim_start_matches("ppt/")));
        slide_rids.push(id);
    }
    let mut notes_master_rid = None;
    if add_notes_master {
        let id = format!("rId{rid}");
        pres_rels.children.push(rel_node(
            &id,
            "notesMaster",
            notes_master.trim_start_matches("ppt/"),
        ));
        notes_master_rid = Some(id);
    }
    parts.insert("ppt/_rels/presentation.xml.rels".into(), to_xml(&pres_rels));

    let mut pres = template.xml("ppt/presentation.xml")?;
    pres.children
        .retain(|c| !matches!(c.local(), "sldIdLst" | "custShowLst"));
    if let Some(ext_lst) = pres.children.iter_mut().find(|c| c.local() == "extLst") {
        ext_lst
            .children
            .retain(|e| !e.children.iter().any(|c| c.local() == "sectionLst"));
    }
    pres.children
        .retain(|c| !(c.local() == "extLst" && c.children.is_empty()));
    let prefix = pres
        .name
        .split_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"));
    let r_prefix = pres
        .attrs
        .iter()
        .find(|(k, v)| k.starts_with("xmlns:") && v == NS_R)
        .map_or("r".to_string(), |(k, _)| {
            k.trim_start_matches("xmlns:").to_string()
        });
    let sld_list = Node {
        name: format!("{prefix}sldIdLst"),
        children: slide_rids
            .iter()
            .enumerate()
            .map(|(i, id)| Node {
                name: format!("{prefix}sldId"),
                attrs: vec![
                    ("id".into(), (256 + i).to_string()),
                    (format!("{r_prefix}:id"), id.clone()),
                ],
                ..Node::default()
            })
            .collect(),
        ..Node::default()
    };
    let after = |names: &[&str], pres: &Node| {
        pres.children
            .iter()
            .rposition(|c| names.contains(&c.local()))
            .map_or(0, |i| i + 1)
    };
    if let Some(id) = notes_master_rid {
        let at = after(&["sldMasterIdLst"], &pres);
        pres.children.insert(
            at,
            Node {
                name: format!("{prefix}notesMasterIdLst"),
                children: vec![Node {
                    name: format!("{prefix}notesMasterId"),
                    attrs: vec![(format!("{r_prefix}:id"), id)],
                    ..Node::default()
                }],
                ..Node::default()
            },
        );
    }
    if !slide_rids.is_empty() {
        let at = after(
            &["sldMasterIdLst", "notesMasterIdLst", "handoutMasterIdLst"],
            &pres,
        );
        pres.children.insert(at, sld_list);
    }
    parts.insert("ppt/presentation.xml".into(), to_xml(&pres));

    // 5. Content types.
    let mut ct = xml::parse(
        parts
            .get("[Content_Types].xml")
            .ok_or_else(|| Error::Pptx("the template has no [Content_Types].xml".into()))?,
    )?;
    ct.children.retain(|c| {
        c.local() != "Override"
            || c.attr("PartName")
                .is_some_and(|p| parts.contains_key(p.trim_start_matches('/')))
    });
    for (name, kind) in new_overrides {
        if !ct
            .children
            .iter()
            .any(|c| c.attr("PartName") == Some(name.as_str()))
        {
            ct.children.push(Node {
                name: "Override".into(),
                attrs: vec![
                    ("PartName".into(), name),
                    ("ContentType".into(), kind.into()),
                ],
                ..Node::default()
            });
        }
    }
    let mut exts: Vec<(String, &str)> = Vec::new();
    for name in parts.keys() {
        let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) else {
            continue;
        };
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpeg" | "jpg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tiff" | "tif" => "image/tiff",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "emf" => "image/x-emf",
            "wmf" => "image/x-wmf",
            _ => continue,
        };
        if !exts.iter().any(|(e, _)| *e == ext) {
            exts.push((ext, mime));
        }
    }
    for (ext, mime) in exts {
        let known = ct.children.iter().any(|c| {
            c.local() == "Default"
                && c.attr("Extension")
                    .is_some_and(|e| e.eq_ignore_ascii_case(&ext))
        });
        if !known {
            ct.children.insert(
                0,
                Node {
                    name: "Default".into(),
                    attrs: vec![
                        ("Extension".into(), ext),
                        ("ContentType".into(), mime.into()),
                    ],
                    ..Node::default()
                },
            );
        }
    }
    // Defaults before Overrides, as the schema orders them.
    ct.children
        .sort_by_key(|c| u8::from(c.local() != "Default"));
    parts.insert("[Content_Types].xml".into(), to_xml(&ct));

    // 6. Package relationships and properties.
    if let Some(bytes) = parts.get("_rels/.rels") {
        let mut root = xml::parse(bytes)?;
        root.children
            .retain(|r| !r.attr("Type").unwrap_or_default().ends_with("/thumbnail"));
        parts.insert("_rels/.rels".into(), to_xml(&root));
    }
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><cp:lastModifiedBy>md2star-rs</cp:lastModifiedBy><cp:revision>1</cp:revision><dcterms:created xsi:type=\"dcterms:W3CDTF\">{FIXED_TIMESTAMP}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{FIXED_TIMESTAMP}</dcterms:modified></cp:coreProperties>",
        esc(&input.title),
        esc(&input.author)
    );
    parts.insert("docProps/core.xml".into(), core.into_bytes());
    let app = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>md2star-rs</Application><PresentationFormat>Custom</PresentationFormat><Slides>{}</Slides><Notes>{}</Notes></Properties>",
        slide_parts.len(),
        notes_parts.len()
    );
    parts.insert("docProps/app.xml".into(), app.into_bytes());

    // 7. Pack: [Content_Types].xml first, then every part in name order.
    pack(&parts)
}

/// Zip `parts` deterministically.
pub fn pack(parts: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let order = std::iter::once("[Content_Types].xml").chain(
        parts
            .keys()
            .map(String::as_str)
            .filter(|k| *k != "[Content_Types].xml"),
    );
    for name in order {
        let Some(bytes) = parts.get(name) else {
            continue;
        };
        writer
            .start_file(name, options)
            .map_err(|e| Error::Pptx(e.to_string()))?;
        writer
            .write_all(bytes)
            .map_err(|e| Error::Pptx(e.to_string()))?;
    }
    let cursor = writer.finish().map_err(|e| Error::Pptx(e.to_string()))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credit_prints_the_site_and_links_the_address() {
        assert_eq!(
            short_credit("Courtesy of https://www.nga.gov/artworks/195605-x."),
            (
                "Courtesy of nga.gov.".to_string(),
                Some("https://www.nga.gov/artworks/195605-x".to_string())
            )
        );
        assert_eq!(short_credit("Photo: Ada"), ("Photo: Ada".to_string(), None));
    }

    #[test]
    fn line_breaks_drop_the_space_and_keep_every_word() {
        let segs = vec![
            Seg {
                text: "alpha beta ".into(),
                ..Seg::default()
            },
            Seg {
                text: "gamma".into(),
                bold: true,
                ..Seg::default()
            },
        ];
        let st = RunStyle {
            lang: "en-US",
            sz: None,
            bold: false,
            italic: false,
            color: None,
            mono: "Consolas",
        };
        let xml = runs_xml(&segs, &[6], &st, &mut Rels::default());
        assert!(xml.contains("<a:t>alpha</a:t></a:r><a:br>"), "{xml}");
        assert!(xml.contains("<a:t>beta </a:t>"), "{xml}");
        assert!(xml.contains("b=\"1\""), "{xml}");
    }

    #[test]
    fn the_math_namespace_lands_on_the_root_element() {
        assert_eq!(
            declare_math_ns("<m:oMathPara><m:oMath/></m:oMathPara>"),
            "<m:oMathPara xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:oMath/></m:oMathPara>"
        );
        assert!(
            crate::pptx::xml::parse(declare_math_ns("<m:oMath><m:r/></m:oMath>").as_bytes())
                .is_ok()
        );
    }

    #[test]
    fn the_blank_png_is_a_png() {
        let info = crate::resources::probe(BLANK_PNG).unwrap();
        assert_eq!((info.width_px, info.height_px), (1, 1));
    }
}
