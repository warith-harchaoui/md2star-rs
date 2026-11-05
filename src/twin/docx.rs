//! DOCX → Markdown (see [`crate::twin`] for what it must carry over).
//!
//! Pure XML over the zip: `word/document.xml` for the body, `styles.xml` to tell a
//! heading from a quote from code, `numbering.xml` to tell bullets from numbers,
//! `footnotes.xml` / `endnotes.xml` for the notes, the part's relationships for links
//! and images, `docProps/core.xml` for the title and author.
//!
//! What decides each paragraph's kind, in order: its style (by name, then by the
//! `basedOn` chain, so a template's "Heading 2 Custom" is still a level-2 heading), its
//! outline level, its numbering, then its content (every run monospace → code; a lone
//! picture → a figure; a lone equation → display math).

use std::collections::{BTreeMap, HashMap, HashSet};

use super::math;
use super::md::{self, Fmt, Piece};
use super::xml::{Node, Package, Rel};
use super::{Twin, TwinOptions};
use crate::ast::{Align, Warning};
use crate::error::{Error, Result};

/// Read a `.docx` back into Markdown.
///
/// # Errors
///
/// [`Error::Read`] when the bytes are not a readable `.docx` (not a zip, no
/// `word/document.xml`, malformed XML).
pub fn to_markdown(docx: &[u8], options: &TwinOptions) -> Result<Twin> {
    let pkg = Package::open(docx)?;
    let main = main_part(&pkg)?;
    let body_root = pkg
        .xml(&main)?
        .ok_or_else(|| Error::Read(format!("missing part {main}")))?;
    let body = body_root
        .child("body")
        .ok_or_else(|| Error::Read("word/document.xml has no <w:body>".into()))?;
    let dir = main.rsplit_once('/').map(|(d, _)| d).unwrap_or("word");
    let mut cx = Cx {
        pkg: &pkg,
        rels: pkg.rels(&main)?,
        styles: Styles::read(&pkg, &format!("{dir}/styles.xml"))?,
        numbering: Numbering::read(&pkg, &format!("{dir}/numbering.xml"))?,
        options,
        media: Vec::new(),
        media_by_part: HashMap::new(),
        used_names: HashSet::new(),
        warnings: Vec::new(),
        warned: HashSet::new(),
        notes: Notes {
            footnotes: read_notes(&pkg, &format!("{dir}/footnotes.xml"), "footnote")?,
            endnotes: read_notes(&pkg, &format!("{dir}/endnotes.xml"), "endnote")?,
            foot_rels: pkg.rels(&format!("{dir}/footnotes.xml"))?,
            end_rels: pkg.rels(&format!("{dir}/endnotes.xml"))?,
            order: Vec::new(),
        },
        meta: Meta::default(),
        list_counters: HashMap::new(),
        dropped_images: 0,
    };
    if let Some(core) = pkg.xml("docProps/core.xml")? {
        cx.meta.core_title = core
            .child("title")
            .map(Node::text)
            .filter(|s| !s.trim().is_empty());
        cx.meta.core_author = core
            .child("creator")
            .map(Node::text)
            .filter(|s| !s.trim().is_empty());
    }
    let paras = cx.blocks(body);
    let mut blocks = assemble(paras);
    // Footnote definitions, in the order of their first reference. A note may itself
    // reference another, so the list can grow while it is drained.
    let mut done = 0;
    while done < cx.notes.order.len() {
        let (kind, id) = cx.notes.order[done].clone();
        done += 1;
        let content = cx.note_blocks(kind, &id);
        blocks.push(md::footnote(&done.to_string(), &content));
    }
    if cx.dropped_images > 0 {
        cx.warnings.push(Warning::new(
            "twin-images-dropped",
            format!("{} image(s) left out (images disabled)", cx.dropped_images),
        ));
    }
    let mut markdown = cx.meta.front_matter(cx.styles.lang.as_deref());
    markdown.push_str(&blocks.join("\n\n"));
    markdown.push('\n');
    Ok(Twin {
        markdown,
        media: cx.media,
        warnings: cx.warnings,
    })
}

/// The main document part, from the package relationships or the usual path.
fn main_part(pkg: &Package) -> Result<String> {
    if let Some(rels) = pkg.xml("_rels/.rels")? {
        for rel in rels.children_named("Relationship") {
            if rel
                .attr("Type")
                .is_some_and(|t| t.ends_with("/officeDocument"))
            {
                if let Some(target) = rel.attr("Target") {
                    let t = target.trim_start_matches('/').to_string();
                    if t.starts_with("word/") && pkg.has(&t) {
                        return Ok(t);
                    }
                }
            }
        }
    }
    if pkg.has("word/document.xml") {
        Ok("word/document.xml".into())
    } else if pkg.has("ppt/presentation.xml") {
        Err(Error::Read(
            "this is a PowerPoint deck, not a Word document; read it as .pptx".into(),
        ))
    } else {
        Err(Error::Read(
            "not a Word document: no word/document.xml".into(),
        ))
    }
}

// ---------------------------------------------------------------------------------
// Styles and numbering
// ---------------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Style {
    name: String,
    based_on: Option<String>,
    outline: Option<u8>,
    num: Option<(String, u8)>,
    bold: bool,
    italic: bool,
    mono: bool,
    strike: bool,
}

#[derive(Default)]
struct Styles {
    by_id: HashMap<String, Style>,
    lang: Option<String>,
}

/// What a paragraph style says the paragraph is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Heading(u8),
    Title,
    Subtitle,
    Author,
    Date,
    Abstract,
    Code,
    Quote,
    Caption,
    Body,
}

static EMPTY_STYLE: Style = Style {
    name: String::new(),
    based_on: None,
    outline: None,
    num: None,
    bold: false,
    italic: false,
    mono: false,
    strike: false,
};

impl Styles {
    fn read(pkg: &Package, path: &str) -> Result<Self> {
        let mut styles = Styles::default();
        let Some(root) = pkg.xml(path)? else {
            return Ok(styles);
        };
        styles.lang = root
            .path(&["docDefaults", "rPrDefault", "rPr", "lang"])
            .and_then(|l| l.attr("val"))
            .map(str::to_string);
        for s in root.children_named("style") {
            let Some(id) = s.attr("styleId") else {
                continue;
            };
            let ppr = s.child("pPr");
            let rpr = s.child("rPr");
            let num = ppr.and_then(|p| p.child("numPr")).and_then(|n| {
                let id = n.child_val("numId")?.to_string();
                let lvl = n
                    .child_val("ilvl")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                Some((id, lvl))
            });
            styles.by_id.insert(
                id.to_string(),
                Style {
                    name: s.child_val("name").unwrap_or(id).to_ascii_lowercase(),
                    based_on: s.child_val("basedOn").map(str::to_string),
                    outline: ppr
                        .and_then(|p| p.child_val("outlineLvl"))
                        .and_then(|v| v.parse().ok()),
                    num,
                    bold: rpr.is_some_and(|r| r.flag("b")),
                    italic: rpr.is_some_and(|r| r.flag("i")),
                    strike: rpr.is_some_and(|r| r.flag("strike") || r.flag("dstrike")),
                    mono: rpr.is_some_and(|r| is_mono_fonts(r.child("rFonts"))),
                },
            );
        }
        Ok(styles)
    }

    /// The style and its ancestors, nearest first (cycle-safe).
    fn chain<'s>(&'s self, id: &'s str) -> Vec<(&'s str, &'s Style)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = Some(id);
        while let Some(i) = cur {
            if !seen.insert(i) {
                break;
            }
            match self.by_id.get_key_value(i) {
                Some((k, s)) => {
                    out.push((k.as_str(), s));
                    cur = s.based_on.as_deref();
                }
                None => {
                    if out.is_empty() {
                        // An undeclared id still says something by its name.
                        out.push((i, &EMPTY_STYLE));
                    }
                    break;
                }
            }
        }
        out
    }

    fn role(&self, id: Option<&str>, outline: Option<u8>) -> Role {
        if let Some(id) = id {
            for (sid, style) in self.chain(id) {
                for candidate in [normalise(&style.name), normalise(sid)] {
                    if candidate.is_empty() {
                        continue;
                    }
                    if let Some(level) = candidate
                        .strip_prefix("heading")
                        .and_then(|n| n.parse::<u8>().ok())
                    {
                        return Role::Heading(level.clamp(1, 6));
                    }
                    let role = match candidate.as_str() {
                        "title" => Some(Role::Title),
                        "subtitle" => Some(Role::Subtitle),
                        "author" => Some(Role::Author),
                        "date" => Some(Role::Date),
                        "abstract" => Some(Role::Abstract),
                        "sourcecode" | "htmlpreformatted" | "code" | "codeblock" | "verbatim"
                        | "plaintext" => Some(Role::Code),
                        "quote" | "blocktext" | "intensequote" | "blockquote" => Some(Role::Quote),
                        "caption" | "imagecaption" | "tablecaption" | "figurecaption" => {
                            Some(Role::Caption)
                        }
                        _ => None,
                    };
                    if let Some(role) = role {
                        return role;
                    }
                }
                if let Some(level) = style.outline {
                    if level < 9 {
                        return Role::Heading((level + 1).min(6));
                    }
                }
            }
        }
        match outline {
            Some(level) if level < 9 => Role::Heading((level + 1).min(6)),
            _ => Role::Body,
        }
    }

    fn num(&self, id: &str) -> Option<(String, u8)> {
        self.chain(id).into_iter().find_map(|(_, s)| s.num.clone())
    }

    /// Formatting a character style contributes, and whether it is monospace.
    fn run_fmt(&self, id: &str) -> (Fmt, bool) {
        let mut fmt = Fmt::default();
        let mut mono = false;
        for (sid, s) in self.chain(id) {
            let key = normalise(sid);
            fmt.bold |= s.bold || key == "strong";
            fmt.italic |= s.italic || key == "emphasis";
            fmt.strike |= s.strike;
            mono |= s.mono || key == "verbatimchar" || key == "sourcecodechar" || key == "code";
        }
        (fmt, mono)
    }
}

fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Default)]
struct Numbering {
    /// numId → (abstractNumId, per-level start overrides).
    nums: HashMap<String, (String, HashMap<u8, u64>)>,
    /// abstractNumId → level → (ordered, start).
    abstracts: HashMap<String, HashMap<u8, (bool, u64)>>,
}

impl Numbering {
    fn read(pkg: &Package, path: &str) -> Result<Self> {
        let mut out = Numbering::default();
        let Some(root) = pkg.xml(path)? else {
            return Ok(out);
        };
        for a in root.children_named("abstractNum") {
            let Some(id) = a.attr("abstractNumId") else {
                continue;
            };
            let mut levels = HashMap::new();
            for lvl in a.children_named("lvl") {
                let ilvl: u8 = lvl.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                let fmt = lvl.child_val("numFmt").unwrap_or("decimal");
                let ordered = !matches!(fmt, "bullet" | "none");
                let start = lvl
                    .child_val("start")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                levels.insert(ilvl, (ordered, start));
            }
            out.abstracts.insert(id.to_string(), levels);
        }
        for n in root.children_named("num") {
            let (Some(id), Some(abs)) = (n.attr("numId"), n.child_val("abstractNumId")) else {
                continue;
            };
            let mut overrides = HashMap::new();
            for o in n.children_named("lvlOverride") {
                let ilvl: u8 = o.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(start) = o.child_val("startOverride").and_then(|v| v.parse().ok()) {
                    overrides.insert(ilvl, start);
                }
            }
            out.nums
                .insert(id.to_string(), (abs.to_string(), overrides));
        }
        Ok(out)
    }

    /// (ordered, start) of a list level; bullets when unknown.
    fn level(&self, num_id: &str, ilvl: u8) -> (bool, u64) {
        let Some((abs, overrides)) = self.nums.get(num_id) else {
            return (false, 1);
        };
        let (ordered, start) = self
            .abstracts
            .get(abs)
            .and_then(|l| l.get(&ilvl))
            .copied()
            .unwrap_or((false, 1));
        (ordered, overrides.get(&ilvl).copied().unwrap_or(start))
    }
}

fn is_mono_fonts(fonts: Option<&Node>) -> bool {
    let Some(f) = fonts else { return false };
    ["ascii", "hAnsi"]
        .iter()
        .filter_map(|k| f.attr(k))
        .any(is_mono_name)
}

/// Is this font family a monospace one? By name: there is no metric to read.
pub(crate) fn is_mono_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("mono")
        || n.contains("courier")
        || n.contains("consolas")
        || n.contains("menlo")
        || n.contains("monaco")
        || n.contains("source code")
        || n.contains("fira code")
        || n.contains("cascadia")
        || n.contains("lucida console")
        || n.contains("lucida sans typewriter")
}

// ---------------------------------------------------------------------------------
// Notes and metadata
// ---------------------------------------------------------------------------------

struct Notes {
    footnotes: HashMap<String, Node>,
    endnotes: HashMap<String, Node>,
    foot_rels: BTreeMap<String, Rel>,
    end_rels: BTreeMap<String, Rel>,
    /// (kind, id) in order of first reference; the label is the position + 1.
    order: Vec<(NoteKind, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteKind {
    Foot,
    End,
}

fn read_notes(pkg: &Package, path: &str, tag: &str) -> Result<HashMap<String, Node>> {
    let mut out = HashMap::new();
    if let Some(root) = pkg.xml(path)? {
        for n in root.children_named(tag) {
            if matches!(
                n.attr("type"),
                Some("separator") | Some("continuationSeparator") | Some("continuationNotice")
            ) {
                continue;
            }
            if let Some(id) = n.attr("id") {
                out.insert(id.to_string(), n.clone());
            }
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Meta {
    title: Option<String>,
    subtitle: Vec<String>,
    authors: Vec<String>,
    date: Option<String>,
    abstract_text: Vec<String>,
    core_title: Option<String>,
    core_author: Option<String>,
}

impl Meta {
    fn front_matter(&self, lang: Option<&str>) -> String {
        let mut lines = Vec::new();
        let title = self.title.clone().or_else(|| self.core_title.clone());
        if let Some(t) = &title {
            lines.push(format!("title: {}", md::yaml_scalar(t)));
        }
        // md2star (Python) sets the author and the date as two "Subtitle" lines: the
        // one equal to the core author is the author, a date-looking one is the date.
        let mut authors = self.authors.clone();
        let mut date = self.date.clone();
        let mut subtitles = Vec::new();
        let n = self.subtitle.len();
        let ends_with_date = n >= 2 && looks_like_date(&self.subtitle[n - 1]);
        for (i, s) in self.subtitle.iter().enumerate() {
            let before_date = ends_with_date && i + 2 == n;
            if authors.is_empty()
                && (self.core_author.as_deref() == Some(s.as_str())
                    || (before_date && looks_like_name(s)))
            {
                authors.push(s.clone());
            } else if date.is_none() && looks_like_date(s) {
                date = Some(s.clone());
            } else {
                subtitles.push(s.clone());
            }
        }
        if authors.is_empty() && title.is_some() {
            if let Some(a) = &self.core_author {
                authors.push(a.clone());
            }
        }
        if !subtitles.is_empty() {
            lines.push(format!(
                "subtitle: {}",
                md::yaml_scalar(&subtitles.join(" — "))
            ));
        }
        match authors.as_slice() {
            [] => {}
            [one] => lines.push(format!("author: {}", md::yaml_scalar(one))),
            many => {
                lines.push("author:".into());
                for a in many {
                    lines.push(format!("  - {}", md::yaml_scalar(a)));
                }
            }
        }
        if let Some(d) = &date {
            lines.push(format!("date: {}", md::yaml_scalar(d)));
        }
        if !self.abstract_text.is_empty() {
            lines.push(format!(
                "abstract: {}",
                md::yaml_scalar(&self.abstract_text.join(" "))
            ));
        }
        if lines.is_empty() {
            return String::new();
        }
        if let Some(l) = lang {
            lines.push(format!("lang: {}", md::yaml_scalar(l)));
        }
        format!("---\n{}\n---\n\n", lines.join("\n"))
    }
}

/// "Ada Lovelace", "A. Turing & G. Hopper": capitalised words, no digits, short.
fn looks_like_name(s: &str) -> bool {
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty() && !matches!(*w, "&" | "and" | "et"))
        .collect();
    !words.is_empty()
        && words.len() <= 8
        && !s.chars().any(|c| c.is_ascii_digit())
        && words
            .iter()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase))
}

fn looks_like_date(s: &str) -> bool {
    const MONTHS: &[&str] = &[
        "jan", "feb", "fév", "mar", "apr", "avr", "may", "mai", "jun", "juin", "jul", "juil",
        "aug", "août", "sep", "oct", "nov", "dec", "déc",
    ];
    let digits = s.chars().filter(char::is_ascii_digit).count();
    let lower = s.to_lowercase();
    digits > 0 && (digits >= 4 || MONTHS.iter().any(|m| lower.contains(m)))
}

// ---------------------------------------------------------------------------------
// Body
// ---------------------------------------------------------------------------------

/// One recovered paragraph-level element, before neighbours are grouped.
#[derive(Debug, Clone)]
enum Para {
    Heading(u8, String),
    Text(String),
    Code(String),
    Quote(String),
    Item {
        level: u8,
        ordered: bool,
        number: u64,
        text: String,
        /// The numbering instance: a new one at level 0 starts a new list.
        list: String,
    },
    Caption(String),
    /// A lone picture: (alt, src).
    Figure(String, String),
    Table(Vec<Vec<String>>, Vec<Align>),
    Rule,
    /// Already Markdown (display math).
    Raw(String),
}

struct Cx<'a> {
    pkg: &'a Package,
    rels: BTreeMap<String, Rel>,
    styles: Styles,
    numbering: Numbering,
    options: &'a TwinOptions,
    media: Vec<(String, Vec<u8>)>,
    media_by_part: HashMap<String, String>,
    used_names: HashSet<String>,
    warnings: Vec<Warning>,
    warned: HashSet<String>,
    notes: Notes,
    meta: Meta,
    /// (numId, level) → last number given.
    list_counters: HashMap<(String, u8), u64>,
    dropped_images: usize,
}

#[derive(Default)]
struct RunStats {
    text_runs: usize,
    mono_runs: usize,
    images: usize,
    math: bool,
    display_math: bool,
}

#[derive(Default)]
struct Field {
    instr: String,
    in_result: bool,
    link: bool,
}

impl Cx<'_> {
    fn warn_once(&mut self, code: &'static str, key: &str, message: String) {
        if self.warned.insert(format!("{code}:{key}")) {
            self.warnings.push(Warning::new(code, message));
        }
    }

    fn blocks(&mut self, container: &Node) -> Vec<Para> {
        let mut out = Vec::new();
        for child in container.elems() {
            match child.local() {
                "p" => self.paragraph(child, &mut out),
                "tbl" => {
                    let para = self.table(child);
                    out.push(para);
                }
                "sdt" => {
                    let is_toc = child
                        .path(&["sdtPr", "docPartObj", "docPartGallery"])
                        .and_then(|g| g.attr("val"))
                        .is_some_and(|v| v.to_ascii_lowercase().contains("table of contents"));
                    if is_toc {
                        continue;
                    }
                    if let Some(content) = child.child("sdtContent") {
                        out.extend(self.blocks(content));
                    }
                }
                "customXml" | "ins" | "smartTag" => out.extend(self.blocks(child)),
                "AlternateContent" => {
                    if let Some(c) = child.child("Choice").or_else(|| child.child("Fallback")) {
                        out.extend(self.blocks(c));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn paragraph(&mut self, p: &Node, out: &mut Vec<Para>) {
        let ppr = p.child("pPr");
        let style = ppr.and_then(|x| x.child_val("pStyle")).map(str::to_string);
        let outline = ppr
            .and_then(|x| x.child_val("outlineLvl"))
            .and_then(|v| v.parse().ok());
        let role = self.styles.role(style.as_deref(), outline);
        let mut pieces = Vec::new();
        let mut stats = RunStats::default();
        self.inlines(p, Fmt::default(), &mut pieces, &mut stats);
        // Display math alone in its paragraph.
        if stats.display_math && stats.text_runs == 0 && stats.images == 0 {
            let tex: Vec<String> = pieces
                .iter()
                .filter_map(|x| match x {
                    Piece::Raw(r) => Some(r.clone()),
                    _ => None,
                })
                .collect();
            out.push(Para::Raw(tex.join("\n\n")));
            return;
        }
        let all_mono = stats.text_runs > 0
            && stats.mono_runs == stats.text_runs
            && stats.images == 0
            && !stats.math
            && !pieces
                .iter()
                .any(|x| matches!(x, Piece::Link(..) | Piece::Raw(_)));
        if role == Role::Code || (role == Role::Body && all_mono && !has_num(ppr)) {
            out.push(Para::Code(code_text(&pieces)));
            return;
        }
        let text = md::inline(&pieces, false);
        // A picture alone in its paragraph is a figure.
        if stats.images == 1 && stats.text_runs == 0 {
            let img = pieces.iter().find_map(|x| match x {
                Piece::Raw(r) if r.starts_with("![") => split_image(r),
                _ => None,
            });
            if let Some((alt, src)) = img {
                out.push(Para::Figure(alt, src));
                return;
            }
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            if ppr.is_some_and(|x| x.path(&["pBdr", "bottom"]).is_some()) || has_hr(p) {
                out.push(Para::Rule);
            }
            return;
        }
        if trimmed.chars().count() >= 3
            && trimmed
                .chars()
                .all(|c| matches!(c, '—' | '–' | '-' | '─' | '\\' | '_' | '*'))
            && plain(&pieces)
                .chars()
                .all(|c| matches!(c, '—' | '–' | '-' | '─' | '_' | '*'))
        {
            out.push(Para::Rule);
            return;
        }
        match role {
            Role::Heading(level) => out.push(Para::Heading(level, trimmed.to_string())),
            Role::Title => self.meta.title = Some(plain(&pieces)),
            Role::Subtitle => self.meta.subtitle.push(plain(&pieces)),
            Role::Author => self.meta.authors.push(plain(&pieces)),
            Role::Date => self.meta.date = Some(plain(&pieces)),
            Role::Abstract => self.meta.abstract_text.push(plain(&pieces)),
            Role::Quote => out.push(Para::Quote(text)),
            Role::Caption => out.push(Para::Caption(trimmed.to_string())),
            Role::Code => out.push(Para::Code(code_text(&pieces))),
            Role::Body => {
                let num = ppr
                    .and_then(|x| x.child("numPr"))
                    .and_then(|n| {
                        let id = n.child_val("numId")?.to_string();
                        let lvl = n
                            .child_val("ilvl")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        Some((id, lvl))
                    })
                    .or_else(|| style.as_deref().and_then(|s| self.styles.num(s)));
                match num {
                    Some((id, level)) if id != "0" => {
                        let (ordered, start) = self.numbering.level(&id, level);
                        // A new item at `level` restarts every deeper level.
                        self.list_counters
                            .retain(|(nid, l), _| !(nid == &id && *l > level));
                        let counter = self.list_counters.entry((id.clone(), level)).or_insert(0);
                        let number = if *counter == 0 { start } else { *counter + 1 };
                        *counter = number;
                        out.push(Para::Item {
                            level,
                            ordered,
                            number,
                            text: task_box(trimmed),
                            list: id,
                        });
                    }
                    _ => out.push(Para::Text(text)),
                }
            }
        }
    }

    fn inlines(&mut self, node: &Node, fmt: Fmt, out: &mut Vec<Piece>, stats: &mut RunStats) {
        let mut field: Option<Field> = None;
        let mut link_stack: Vec<(String, Vec<Piece>)> = Vec::new();
        for child in node.elems() {
            match child.local() {
                "r" => {
                    // Complex fields: HYPERLINK "url" wraps the runs of its result.
                    if let Some(fc) = child.child("fldChar") {
                        match fc.attr("fldCharType") {
                            Some("begin") => field = Some(Field::default()),
                            Some("separate") => {
                                if let Some(f) = field.as_mut() {
                                    f.in_result = true;
                                    if let Some(url) = hyperlink_instr(&f.instr) {
                                        f.link = true;
                                        link_stack.push((url, Vec::new()));
                                    }
                                }
                            }
                            Some("end") => {
                                let was_link = field.take().is_some_and(|f| f.link);
                                if let Some((url, inner)) = link_stack.pop_if(|_| was_link) {
                                    target(&mut link_stack, out).push(Piece::Link(url, inner));
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(f) = field.as_mut() {
                        if !f.in_result {
                            for it in child.children_named("instrText") {
                                f.instr.push_str(&it.text());
                            }
                            continue;
                        }
                    }
                    let mut run = Vec::new();
                    self.run(child, fmt, &mut run, stats);
                    target(&mut link_stack, out).extend(run);
                }
                "hyperlink" => {
                    let url = child
                        .attr("id")
                        .and_then(|id| self.rels.get(id))
                        .map(|r| r.target.clone())
                        .or_else(|| child.attr("anchor").map(|a| format!("#{a}")));
                    let mut inner = Vec::new();
                    self.inlines(child, fmt, &mut inner, stats);
                    let dest = target(&mut link_stack, out);
                    match url {
                        Some(u) => dest.push(Piece::Link(u, inner)),
                        None => dest.extend(inner),
                    }
                }
                "fldSimple" => {
                    let mut inner = Vec::new();
                    self.inlines(child, fmt, &mut inner, stats);
                    let dest = target(&mut link_stack, out);
                    match child.attr("instr").and_then(hyperlink_instr) {
                        Some(u) => dest.push(Piece::Link(u, inner)),
                        None => dest.extend(inner),
                    }
                }
                "ins" | "smartTag" | "customXml" | "dir" | "bdo" => {
                    let mut inner = Vec::new();
                    self.inlines(child, fmt, &mut inner, stats);
                    target(&mut link_stack, out).extend(inner);
                }
                "sdt" => {
                    if let Some(c) = child.child("sdtContent") {
                        let mut inner = Vec::new();
                        self.inlines(c, fmt, &mut inner, stats);
                        target(&mut link_stack, out).extend(inner);
                    }
                }
                "oMath" => {
                    let tex = self.math(child);
                    stats.math = true;
                    target(&mut link_stack, out).push(Piece::Raw(format!("${tex}$")));
                }
                "oMathPara" => {
                    stats.math = true;
                    stats.display_math = true;
                    for m in child.children_named("oMath") {
                        let tex = self.math(m);
                        target(&mut link_stack, out).push(Piece::Raw(format!("$$\n{tex}\n$$")));
                    }
                }
                _ => {}
            }
        }
        // An unterminated field: keep what it showed.
        while let Some((url, inner)) = link_stack.pop() {
            target(&mut link_stack, out).push(Piece::Link(url, inner));
        }
    }

    fn math(&mut self, node: &Node) -> String {
        let (tex, unknown) = math::to_latex(node);
        for u in unknown {
            self.warn_once(
                "twin-math-unknown",
                &u,
                format!("equation element `{u}` has no LaTeX equivalent here; its text was kept"),
            );
        }
        tex
    }

    fn run(&mut self, r: &Node, base: Fmt, out: &mut Vec<Piece>, stats: &mut RunStats) {
        let rpr = r.child("rPr");
        let mut fmt = base;
        let mut mono = false;
        if let Some(style) = rpr.and_then(|p| p.child_val("rStyle")) {
            let (sf, sm) = self.styles.run_fmt(style);
            fmt.bold |= sf.bold;
            fmt.italic |= sf.italic;
            fmt.strike |= sf.strike;
            mono |= sm;
        }
        if let Some(p) = rpr {
            if p.child("b").is_some() {
                fmt.bold = p.flag("b");
            }
            if p.child("i").is_some() {
                fmt.italic = p.flag("i");
            }
            if p.child("strike").is_some() || p.child("dstrike").is_some() {
                fmt.strike = p.flag("strike") || p.flag("dstrike");
            }
            match p.child_val("vertAlign") {
                Some("superscript") => fmt.sup = true,
                Some("subscript") => fmt.sub = true,
                _ => {}
            }
            mono |= is_mono_fonts(p.child("rFonts"));
        }
        if mono {
            fmt.code = true;
        }
        let mut has_text = false;
        for c in r.elems() {
            match c.local() {
                "t" => {
                    has_text = true;
                    out.push(Piece::Text(c.text(), fmt));
                }
                "tab" | "ptab" => {
                    has_text = true;
                    out.push(Piece::Text(if mono { "\t" } else { " " }.into(), fmt));
                }
                "noBreakHyphen" => out.push(Piece::Text("-".into(), fmt)),
                "sym" => {
                    if let Some(ch) = c
                        .attr("char")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .and_then(char::from_u32)
                    {
                        out.push(Piece::Text(ch.to_string(), fmt));
                    }
                }
                "br" if !matches!(c.attr("type"), Some("page") | Some("column")) => {
                    out.push(Piece::Break);
                }
                "cr" => out.push(Piece::Break),
                "footnoteReference" | "endnoteReference" => {
                    let kind = if c.is("footnoteReference") {
                        NoteKind::Foot
                    } else {
                        NoteKind::End
                    };
                    if let Some(id) = c.attr("id") {
                        let key = (kind, id.to_string());
                        let pos = match self.notes.order.iter().position(|k| *k == key) {
                            Some(p) => p,
                            None => {
                                self.notes.order.push(key);
                                self.notes.order.len() - 1
                            }
                        };
                        out.push(Piece::Raw(format!("[^{}]", pos + 1)));
                    }
                }
                "drawing" | "pict" | "object" => {
                    if let Some(img) = self.image(c) {
                        stats.images += 1;
                        out.push(Piece::Raw(img));
                    }
                }
                "AlternateContent" => {
                    if let Some(a) = c.child("Choice").or_else(|| c.child("Fallback")) {
                        let wrapper = Node {
                            name: "w:r".into(),
                            attrs: Vec::new(),
                            children: a.children.clone(),
                        };
                        self.run(&wrapper, base, out, stats);
                    }
                }
                _ => {}
            }
        }
        if has_text {
            stats.text_runs += 1;
            if mono {
                stats.mono_runs += 1;
            }
        }
    }

    /// `![alt](media/…)` for a picture, extracting its bytes.
    fn image(&mut self, node: &Node) -> Option<String> {
        let (rid, linked) = match (node.find("blip"), node.find("imagedata")) {
            (Some(b), _) => match b.attr("embed") {
                Some(e) => (e.to_string(), false),
                None => (b.attr("link")?.to_string(), true),
            },
            (None, Some(i)) => (i.attr("id")?.to_string(), false),
            _ => return None,
        };
        if !self.options.images {
            self.dropped_images += 1;
            return None;
        }
        let alt = node
            .find("docPr")
            .and_then(|d| {
                d.attr("descr")
                    .filter(|s| !s.is_empty())
                    .or_else(|| d.attr("title").filter(|s| !s.is_empty()))
            })
            .unwrap_or("")
            .to_string();
        let rel = self.rels.get(&rid).cloned()?;
        let src = if rel.external || linked {
            rel.target.clone()
        } else {
            self.extract(&rel.target)?
        };
        Some(format!(
            "![{}]({})",
            md::escape(&alt, false, ""),
            md::link_target(&src)
        ))
    }

    /// Copy a media part out of the package once; return its Markdown path.
    fn extract(&mut self, part: &str) -> Option<String> {
        if let Some(p) = self.media_by_part.get(part) {
            return Some(p.clone());
        }
        let Some(bytes) = self.pkg.bytes(part) else {
            self.warn_once(
                "twin-missing-media",
                part,
                format!("image part `{part}` is referenced but missing from the package"),
            );
            return None;
        };
        let path = media_path(&self.options.media_dir, part, &mut self.used_names);
        self.media.push((path.clone(), bytes.to_vec()));
        self.media_by_part.insert(part.to_string(), path.clone());
        Some(path)
    }

    fn table(&mut self, tbl: &Node) -> Para {
        let mut rows = Vec::new();
        let mut aligns: Vec<Align> = Vec::new();
        for (ri, tr) in tbl.children_named("tr").enumerate() {
            let mut row = Vec::new();
            for tc in tr.children_named("tc") {
                let span: usize = tc
                    .path(&["tcPr", "gridSpan"])
                    .and_then(|g| g.attr("val"))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let continued = tc
                    .path(&["tcPr", "vMerge"])
                    .is_some_and(|v| v.attr("val") != Some("restart"));
                let mut parts = Vec::new();
                let mut align = None;
                for child in tc.elems() {
                    match child.local() {
                        "p" => {
                            if align.is_none() {
                                align = child
                                    .path(&["pPr", "jc"])
                                    .and_then(|j| j.attr("val"))
                                    .map(str::to_string);
                            }
                            let mut pieces = Vec::new();
                            let mut stats = RunStats::default();
                            self.inlines(child, Fmt::default(), &mut pieces, &mut stats);
                            let s = md::inline(&pieces, true);
                            if !s.trim().is_empty() {
                                parts.push(s.trim().to_string());
                            }
                        }
                        "tbl" => {
                            self.warn_once(
                                "twin-nested-table",
                                "",
                                "a table nested in a table cell was flattened to text".into(),
                            );
                            if let Para::Table(inner, _) = self.table(child) {
                                parts.push(
                                    inner
                                        .iter()
                                        .map(|r| r.join(", "))
                                        .collect::<Vec<_>>()
                                        .join("; "),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                let col = row.len();
                if ri <= 1 {
                    let a = match align.as_deref() {
                        Some("center") => Align::Center,
                        Some("right") | Some("end") => Align::Right,
                        Some("left") | Some("start") => Align::Left,
                        _ => Align::Default,
                    };
                    if aligns.len() <= col {
                        aligns.resize(col + 1, Align::Default);
                    }
                    if ri == 0 || aligns[col] == Align::Default {
                        aligns[col] = a;
                    }
                }
                row.push(if continued {
                    String::new()
                } else {
                    parts.join(" ")
                });
                for _ in 1..span {
                    row.push(String::new());
                }
            }
            rows.push(row);
        }
        // Writers set the header row in bold; the pipe table's header row already is.
        if let Some(header) = rows.first_mut() {
            let all_bold = header.iter().all(|c| {
                c.is_empty()
                    || (c.len() > 4
                        && c.starts_with("**")
                        && c.ends_with("**")
                        && !c[2..c.len() - 2].contains("**"))
            });
            if all_bold && header.iter().any(|c| !c.is_empty()) {
                for c in header.iter_mut().filter(|c| !c.is_empty()) {
                    *c = c[2..c.len() - 2].to_string();
                }
            }
        }
        // A uniformly left-aligned table is the default look, not an explicit choice.
        if aligns
            .iter()
            .all(|a| matches!(a, Align::Left | Align::Default))
        {
            aligns.iter_mut().for_each(|a| *a = Align::Default);
        }
        Para::Table(rows, aligns)
    }

    fn note_blocks(&mut self, kind: NoteKind, id: &str) -> Vec<String> {
        let (note, rels) = match kind {
            NoteKind::Foot => (
                self.notes.footnotes.get(id).cloned(),
                self.notes.foot_rels.clone(),
            ),
            NoteKind::End => (
                self.notes.endnotes.get(id).cloned(),
                self.notes.end_rels.clone(),
            ),
        };
        let Some(note) = note else {
            return vec![String::new()];
        };
        let saved = std::mem::replace(&mut self.rels, rels);
        let paras = self.blocks(&note);
        self.rels = saved;
        let paras = paras
            .into_iter()
            .map(|p| match p {
                Para::Text(t) => Para::Text(t.trim_start().to_string()),
                other => other,
            })
            .collect();
        let mut out = assemble(paras);
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }
}

/// Where a piece goes: inside the innermost open field hyperlink, or the paragraph.
fn target<'v>(
    stack: &'v mut [(String, Vec<Piece>)],
    out: &'v mut Vec<Piece>,
) -> &'v mut Vec<Piece> {
    match stack.last_mut() {
        Some((_, inner)) => inner,
        None => out,
    }
}

fn has_num(ppr: Option<&Node>) -> bool {
    ppr.and_then(|p| p.child("numPr")).is_some()
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

/// `HYPERLINK "https://…"` (or `HYPERLINK \l "anchor"`) → the destination.
fn hyperlink_instr(instr: &str) -> Option<String> {
    let rest = instr.trim().strip_prefix("HYPERLINK")?.trim();
    let anchor = rest.starts_with("\\l");
    let quoted = rest.split('"').nth(1)?;
    if quoted.is_empty() {
        return None;
    }
    Some(if anchor {
        format!("#{quoted}")
    } else {
        quoted.to_string()
    })
}

fn has_hr(p: &Node) -> bool {
    let mut rects = Vec::new();
    p.find_all("rect", &mut rects);
    rects.iter().any(|r| r.attr("hr") == Some("t"))
}

fn plain(pieces: &[Piece]) -> String {
    let mut s = String::new();
    for p in pieces {
        match p {
            Piece::Text(t, _) => s.push_str(t),
            Piece::Break => s.push(' '),
            Piece::Link(_, inner) => s.push_str(&plain(inner)),
            Piece::Raw(_) => {}
        }
    }
    s.trim().to_string()
}

/// The ☐ / ☑ boxes the writers put in front of task-list items → `[ ]` / `[x]`.
fn task_box(text: &str) -> String {
    for (glyph, mark) in [("☐", "[ ]"), ("☑", "[x]"), ("☒", "[x]")] {
        if let Some(rest) = text.strip_prefix(glyph) {
            return format!("{mark} {}", rest.trim_start());
        }
    }
    text.to_string()
}

/// `![alt](src)` → (alt, src).
fn split_image(md: &str) -> Option<(String, String)> {
    let inner = md.strip_prefix("![")?;
    let close = inner.rfind("](")?;
    let src = inner[close + 2..].strip_suffix(')')?.to_string();
    Some((inner[..close].to_string(), src))
}

/// A unique `<media_dir>/<file name>` for a package part.
pub(crate) fn media_path(dir: &str, part: &str, used: &mut HashSet<String>) -> String {
    let file = part.rsplit('/').next().unwrap_or(part);
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file.to_string(), String::new()),
    };
    let mut candidate = format!("{dir}/{stem}{ext}");
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{dir}/{stem}-{n}{ext}");
        n += 1;
    }
    candidate
}

// ---------------------------------------------------------------------------------
// Grouping paragraphs into Markdown blocks
// ---------------------------------------------------------------------------------

fn assemble(paras: Vec<Para>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < paras.len() {
        match &paras[i] {
            Para::Heading(level, text) => {
                out.push(format!("{} {}", "#".repeat(*level as usize), text));
                i += 1;
            }
            Para::Text(t) => {
                out.push(md::guard_line_start(t));
                i += 1;
            }
            Para::Raw(r) => {
                out.push(r.clone());
                i += 1;
            }
            Para::Rule => {
                out.push("---".into());
                i += 1;
            }
            Para::Code(_) => {
                let mut lines = Vec::new();
                while let Some(Para::Code(l)) = paras.get(i) {
                    lines.push(l.clone());
                    i += 1;
                }
                out.push(md::code_block("", &lines.join("\n")));
            }
            Para::Quote(_) => {
                let mut parts = Vec::new();
                while let Some(Para::Quote(q)) = paras.get(i) {
                    parts.push(md::guard_line_start(q));
                    i += 1;
                }
                out.push(md::quote(&parts.join("\n\n")));
            }
            Para::Item { .. } => {
                let mut lines = Vec::new();
                // Content column of each open level, to indent the next one under it.
                let mut columns: Vec<usize> = Vec::new();
                let mut current: Option<(&str, bool)> = None;
                while let Some(Para::Item {
                    level,
                    ordered,
                    number,
                    text,
                    list,
                }) = paras.get(i)
                {
                    let level = *level as usize;
                    // Another list (instance or kind) at the top level is another block.
                    if level == 0 {
                        if current.is_some_and(|c| c != (list.as_str(), *ordered)) {
                            break;
                        }
                        current = Some((list.as_str(), *ordered));
                    }
                    columns.truncate(level);
                    let indent = columns.last().copied().unwrap_or(0);
                    while columns.len() < level {
                        columns.push(indent);
                    }
                    let marker = if *ordered {
                        format!("{number}. ")
                    } else {
                        "- ".to_string()
                    };
                    let body = if text.starts_with("[ ] ") || text.starts_with("[x] ") {
                        text.clone()
                    } else {
                        md::guard_line_start(text)
                    };
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
            Para::Figure(alt, src) => {
                let mut alt = alt.clone();
                i += 1;
                if let Some(Para::Caption(c)) = paras.get(i) {
                    alt = c.clone();
                    i += 1;
                }
                out.push(format!("![{alt}]({src})"));
            }
            Para::Table(rows, aligns) => {
                let mut table = md::table(rows, aligns);
                i += 1;
                if let Some(Para::Caption(c)) = paras.get(i) {
                    table.push_str(&format!("\n\nTable: {c}"));
                    i += 1;
                }
                out.push(table);
            }
            Para::Caption(c) => {
                // A caption before its table (Word's habit) moves after it.
                if let Some(Para::Table(rows, aligns)) = paras.get(i + 1) {
                    out.push(format!("{}\n\nTable: {c}", md::table(rows, aligns)));
                    i += 2;
                } else {
                    out.push(md::guard_line_start(c));
                    i += 1;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperlink_fields_are_read() {
        assert_eq!(
            hyperlink_instr(r#" HYPERLINK "https://deraison.ai" "#).as_deref(),
            Some("https://deraison.ai")
        );
        assert_eq!(
            hyperlink_instr(r#"HYPERLINK \l "_Toc1""#).as_deref(),
            Some("#_Toc1")
        );
        assert_eq!(hyperlink_instr("PAGEREF x"), None);
    }

    #[test]
    fn media_names_are_unique() {
        let mut used = HashSet::new();
        assert_eq!(media_path("m", "word/media/a.png", &mut used), "m/a.png");
        assert_eq!(media_path("m", "ppt/media/a.png", &mut used), "m/a-2.png");
    }

    #[test]
    fn nested_lists_indent_under_their_parent() {
        let item = |level, ordered, number, t: &str| Para::Item {
            level,
            ordered,
            number,
            text: t.into(),
            list: if level == 0 { "1".into() } else { "2".into() },
        };
        let md = assemble(vec![
            item(0, false, 1, "a"),
            item(1, true, 1, "b"),
            item(1, true, 2, "c"),
            item(0, false, 1, "d"),
        ]);
        assert_eq!(md, vec!["- a\n  1. b\n  2. c\n- d".to_string()]);
    }

    #[test]
    fn captions_attach_to_figures_and_tables() {
        let md = assemble(vec![
            Para::Figure(String::new(), "m/a.png".into()),
            Para::Caption("A photo".into()),
            Para::Caption("Numbers".into()),
            Para::Table(vec![vec!["A".into()], vec!["1".into()]], vec![]),
        ]);
        assert_eq!(md[0], "![A photo](m/a.png)");
        assert_eq!(md[1], "| A |\n|---|\n| 1 |\n\nTable: Numbers");
    }

    #[test]
    fn dates_are_recognised() {
        assert!(looks_like_date("Wednesday, July 15, 2026"));
        assert!(looks_like_date("15 juin 2026"));
        assert!(!looks_like_date("Richard Feynman"));
        assert!(looks_like_name("Richard Feynman"));
        assert!(!looks_like_name("A fixture for the ports"));
    }
}
