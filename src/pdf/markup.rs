//! [`Document`] → Typst markup.
//!
//! Every piece of author text goes through [`escape`] (markup) or [`string`] (a Typst
//! string literal), so nothing an author writes can become Typst code. Structure is
//! written with Typst's own elements — `heading`, `list`/`enum`, `table`, `figure`,
//! `footnote`, `quote`, `raw`, `outline`, math — so the typesetting (hyphenation,
//! justification, footnote placement, table breaking) is Typst's.

use std::collections::{HashMap, HashSet};

use crate::ast::{cite_text, Align, Attr, Block, Document, Image, Inline, Warning};
use crate::options::Options;
use crate::resources::{ImageFormat, Resolver};

use super::math;

/// Generated markup, the files it references, and what could not be carried over.
pub struct Markup {
    /// The Typst source.
    pub source: String,
    /// Virtual path → bytes, for every image the source references.
    pub files: Vec<(String, Vec<u8>)>,
    /// Non-fatal observations.
    pub warnings: Vec<Warning>,
    /// Whether the source holds any formula (the fallback retries without them).
    pub has_math: bool,
}

/// How formulas are written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MathMode {
    /// Native Typst math.
    Native,
    /// Their LaTeX source in a code span (the fallback when native math fails).
    Source,
}

/// The text width of the page, in points, for an A4 page with 2.5 cm margins: an image
/// wider than this is scaled down to the column.
const TEXT_WIDTH_PT: f64 = 453.0;

/// Build the Typst source for `doc`.
pub fn build(doc: &Document, options: &Options, math_mode: MathMode) -> Markup {
    let mut w = Writer {
        out: String::new(),
        files: Vec::new(),
        warnings: Vec::new(),
        resolver: Resolver::from_options(options),
        notes: HashMap::new(),
        note_labels: HashMap::new(),
        labels: HashSet::new(),
        image_count: 0,
        image_paths: HashMap::new(),
        math_mode,
        has_math: false,
    };
    collect_labels(&doc.blocks, &mut w.labels);
    for b in &doc.blocks {
        if let Block::FootnoteDef { label, content } = b {
            w.notes.insert(label.clone(), content.clone());
        }
    }
    w.preamble(doc, options);
    w.title_block(doc, options);
    if options.toc || doc.meta.toc == Some(true) {
        w.out.push_str("#outline()\n\n");
    }
    for b in &doc.blocks {
        w.block(b);
    }
    Markup {
        source: w.out,
        files: w.files,
        warnings: w.warnings,
        has_math: w.has_math,
    }
}

struct Writer {
    out: String,
    files: Vec<(String, Vec<u8>)>,
    warnings: Vec<Warning>,
    resolver: Resolver,
    notes: HashMap<String, Vec<Block>>,
    /// Footnote label → the Typst label of its first use.
    note_labels: HashMap<String, String>,
    /// Heading and figure ids that exist (only those can be linked to).
    labels: HashSet<String>,
    image_count: usize,
    /// Image source → virtual path, so an image used twice is embedded once.
    image_paths: HashMap<String, Option<(String, f64, f64)>>,
    math_mode: MathMode,
    has_math: bool,
}

fn collect_labels(blocks: &[Block], out: &mut HashSet<String>) {
    for b in blocks {
        match b {
            Block::Heading { attr, .. } | Block::Table { attr, .. } => {
                if let Some(id) = attr.id.as_deref().filter(|i| valid_label(i)) {
                    out.insert(id.to_string());
                }
            }
            Block::Figure(img) => {
                if let Some(id) = img.attr.id.as_deref().filter(|i| valid_label(i)) {
                    out.insert(id.to_string());
                }
            }
            Block::BlockQuote(inner) | Block::Div { content: inner, .. } => {
                collect_labels(inner, out)
            }
            Block::List { items, .. } => {
                for i in items {
                    collect_labels(&i.content, out);
                }
            }
            _ => {}
        }
    }
}

/// A label Typst accepts: letters, digits, `_`, `-`, `:`, `.`.
fn valid_label(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
        && !id.ends_with(['.', ':'])
}

/// Escape author text for Typst markup.
pub fn escape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let needs = match c {
            '\\' | '#' | '*' | '_' | '`' | '$' | '<' | '>' | '@' | '[' | ']' | '~' => true,
            '/' => matches!(next, Some('/') | Some('*')),
            _ => false,
        };
        if needs {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape what would turn the start of a line into a heading, a list or an enum item.
fn guard_start(line: String) -> String {
    let first = line.chars().next();
    if matches!(first, Some('=') | Some('+') | Some('-')) {
        return format!("\\{line}");
    }
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() && line[digits.len()..].starts_with('.') {
        return format!("{digits}\\{}", &line[digits.len()..]);
    }
    line
}

/// A Typst string literal.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `fr-FR` → (`fr`, `FR`); invalid parts dropped.
fn lang_region(tag: &str) -> (Option<String>, Option<String>) {
    let mut parts = tag.split(['-', '_']);
    let lang = parts
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|l| (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()));
    let region = parts
        .next()
        .map(str::to_ascii_uppercase)
        .filter(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_alphabetic()));
    (lang, region)
}

impl Writer {
    fn warn(&mut self, code: &'static str, message: String) {
        self.warnings.push(Warning::new(code, message));
    }

    fn preamble(&mut self, doc: &Document, options: &Options) {
        let lang = options.lang.clone().or_else(|| doc.meta.lang.clone());
        let (lang, region) = lang.as_deref().map(lang_region).unwrap_or((None, None));
        let authors: Vec<String> = match &options.author {
            Some(a) => vec![a.clone()],
            None => doc.meta.authors.clone(),
        };
        let mut document = vec!["date: none".to_string()];
        if let Some(t) = &doc.meta.title {
            document.push(format!("title: {}", string(t)));
        }
        if !authors.is_empty() {
            let list: Vec<String> = authors.iter().map(|a| string(a)).collect();
            document.push(format!("author: ({},)", list.join(", ")));
        }
        if !doc.meta.keywords.is_empty() {
            let list: Vec<String> = doc.meta.keywords.iter().map(|k| string(k)).collect();
            document.push(format!("keywords: ({},)", list.join(", ")));
        }
        // US Letter where the document says it is American, A4 everywhere else.
        let paper = if region.as_deref() == Some("US") || region.as_deref() == Some("CA") {
            "us-letter"
        } else {
            "a4"
        };
        let mut text = vec![
            "font: (\"Libertinus Serif\", \"New Computer Modern\")".to_string(),
            "size: 11pt".to_string(),
        ];
        if let Some(l) = &lang {
            text.push(format!("lang: {}", string(l)));
        }
        if let Some(r) = &region {
            text.push(format!("region: {}", string(r)));
        }
        self.out.push_str(&format!(
            "// Generated by md2star-rs. Fonts are embedded; no date is written.\n\
             #set document({})\n\
             #set page(paper: \"{paper}\", margin: (x: 2.5cm, y: 2.5cm), numbering: \"1\")\n\
             #set text({})\n\
             #set par(justify: true)\n\
             #set heading(numbering: none)\n\
             #show heading: set block(above: 1.4em, below: 0.8em)\n\
             #show heading.where(level: 1): set text(size: 1.45em)\n\
             #show heading.where(level: 2): set text(size: 1.25em)\n\
             #show raw: set text(font: \"DejaVu Sans Mono\", size: 0.82em)\n\
             #show raw.where(block: true): block.with(fill: luma(246), inset: 8pt, radius: 2pt, width: 100%)\n\
             #show link: set text(fill: rgb(\"#1f4e8c\"))\n\
             #set table(stroke: 0.5pt + luma(170), inset: 6pt)\n\
             #show table.cell.where(y: 0): strong\n\
             #show math.equation: set text(font: \"New Computer Modern Math\")\n\n",
            document.join(", "),
            text.join(", ")
        ));
    }

    fn title_block(&mut self, doc: &Document, options: &Options) {
        let meta = &doc.meta;
        let authors: Vec<String> = match &options.author {
            Some(a) => vec![a.clone()],
            None => meta.authors.clone(),
        };
        let date = options.date.clone().or_else(|| meta.date.clone());
        if meta.title.is_none() && meta.subtitle.is_none() && authors.is_empty() && date.is_none() {
            return;
        }
        let mut lines = Vec::new();
        if let Some(t) = &meta.title {
            lines.push(format!("#text(size: 2em, weight: \"bold\")[{}]", escape(t)));
        }
        if let Some(s) = &meta.subtitle {
            lines.push(format!("#text(size: 1.3em)[{}]", escape(s)));
        }
        if !authors.is_empty() {
            lines.push(format!(
                "#v(0.6em) {}",
                authors
                    .iter()
                    .map(|a| escape(a))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }
        if let Some(d) = &date {
            lines.push(escape(d));
        }
        self.out.push_str(&format!(
            "#align(center)[\n{}\n]\n#v(1.2em)\n\n",
            lines.join(" \\\n")
        ));
        if let Some(a) = &meta.abstract_text {
            self.out.push_str(&format!(
                "#block(inset: (x: 2em))[#text(size: 0.92em)[{}]]\n\n",
                escape(a)
            ));
        }
    }

    fn block(&mut self, b: &Block) {
        match b {
            Block::Heading {
                level,
                content,
                attr,
            } => {
                let text = self.inlines_one_line(content);
                let label = attr
                    .id
                    .as_deref()
                    .filter(|i| valid_label(i))
                    .map(|i| format!(" <{i}>"))
                    .unwrap_or_default();
                self.out.push_str(&format!(
                    "{} {}{label}\n\n",
                    "=".repeat((*level).clamp(1, 6) as usize),
                    text.trim()
                ));
            }
            Block::Paragraph(inl) => {
                let text = self.inlines(inl);
                if !text.trim().is_empty() {
                    self.out.push_str(&guard_start(text.trim().to_string()));
                    self.out.push_str("\n\n");
                }
            }
            Block::CodeBlock { lang, code, .. } => {
                let lang = lang
                    .as_deref()
                    .filter(|l| {
                        l.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "+-#_".contains(c))
                    })
                    .map(|l| format!("lang: {}, ", string(l)))
                    .unwrap_or_default();
                self.out.push_str(&format!(
                    "#raw(block: true, {lang}{})\n\n",
                    string(code.trim_end_matches('\n'))
                ));
            }
            Block::Mermaid { code, .. } => self.mermaid(code),
            Block::Figure(img) => self.figure(img),
            Block::BlockQuote(inner) => {
                let body = self.nested(inner);
                self.out
                    .push_str(&format!("#quote(block: true)[\n{}]\n\n", body));
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let list = self.list(*ordered, *start, items);
                self.out.push_str(&list);
                self.out.push_str("\n\n");
            }
            Block::Table {
                headers,
                rows,
                aligns,
                caption,
                attr,
            } => self.table(headers, rows, aligns, caption, attr),
            Block::Div { attr, content } => self.div(attr, content),
            // Speaker notes belong to slides.
            Block::Notes(_) => {}
            Block::ThematicBreak => self
                .out
                .push_str("#line(length: 100%, stroke: 0.5pt + luma(160))\n\n"),
            Block::RawHtml(_) => {}
            // Definitions are written where they are referenced.
            Block::FootnoteDef { .. } => {}
        }
    }

    /// Blocks rendered into a string (for content blocks).
    fn nested(&mut self, blocks: &[Block]) -> String {
        let saved = std::mem::take(&mut self.out);
        for b in blocks {
            self.block(b);
        }
        let inner = std::mem::replace(&mut self.out, saved);
        inner.trim_end().to_string() + "\n"
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[crate::ast::ListItem]) -> String {
        let mut parts = Vec::new();
        for item in items {
            let mut body = self.nested(&item.content);
            if let Some(done) = item.task {
                let box_ = if done { "☒" } else { "☐" };
                body = format!("{box_} {body}");
            }
            parts.push(format!("[{}]", body.trim_end()));
        }
        if ordered {
            format!("#enum(start: {start}, {})", parts.join(", "))
        } else {
            format!("#list({})", parts.join(", "))
        }
    }

    fn table(
        &mut self,
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        aligns: &[Align],
        caption: &[Inline],
        attr: &Attr,
    ) {
        let columns = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        let align: Vec<&str> = (0..columns)
            .map(|i| match aligns.get(i).copied().unwrap_or_default() {
                Align::Left | Align::Default => "left",
                Align::Center => "center",
                Align::Right => "right",
            })
            .collect();
        let mut cells = Vec::new();
        let header: Vec<String> = (0..columns)
            .map(|i| {
                let c = headers
                    .get(i)
                    .map(|h| self.inlines_one_line(h))
                    .unwrap_or_default();
                format!("[{c}]")
            })
            .collect();
        cells.push(format!("table.header({})", header.join(", ")));
        for row in rows {
            for i in 0..columns {
                let c = row
                    .get(i)
                    .map(|h| self.inlines_one_line(h))
                    .unwrap_or_default();
                cells.push(format!("[{c}]"));
            }
        }
        let table = format!(
            "table(columns: {columns}, align: ({},), {})",
            align.join(", "),
            cells.join(", ")
        );
        let label = attr
            .id
            .as_deref()
            .filter(|i| valid_label(i))
            .map(|i| format!(" <{i}>"))
            .unwrap_or_default();
        if caption.is_empty() {
            self.out.push_str(&format!("#{table}{label}\n\n"));
        } else {
            let cap = self.inlines_one_line(caption);
            self.out.push_str(&format!(
                "#figure({table}, caption: [{cap}], kind: table){label}\n\n"
            ));
        }
    }

    fn div(&mut self, attr: &Attr, content: &[Block]) {
        if attr.has_class("columns") {
            let columns: Vec<(&Attr, &[Block])> = content
                .iter()
                .filter_map(|b| match b {
                    Block::Div { attr, content } if attr.has_class("column") => {
                        Some((attr, content.as_slice()))
                    }
                    _ => None,
                })
                .collect();
            if !columns.is_empty() {
                let widths: Vec<String> = columns
                    .iter()
                    .map(|(a, _)| match a.get("width") {
                        Some(w) if w.ends_with('%') && w[..w.len() - 1].parse::<f64>().is_ok() => {
                            w.to_string()
                        }
                        _ => "1fr".to_string(),
                    })
                    .collect();
                let bodies: Vec<String> = columns
                    .iter()
                    .map(|(_, blocks)| format!("[\n{}]", self.nested(blocks)))
                    .collect();
                self.out.push_str(&format!(
                    "#grid(columns: ({},), column-gutter: 1.5em, {})\n\n",
                    widths.join(", "),
                    bodies.join(", ")
                ));
                return;
            }
        }
        if attr.has_class("csl-entry") {
            let body = self.nested(content);
            self.out.push_str(&format!(
                "#par(hanging-indent: 1.5em, justify: false)[{}]\n\n",
                body.trim()
            ));
            return;
        }
        for b in content {
            self.block(b);
        }
    }

    fn mermaid(&mut self, code: &str) {
        #[cfg(feature = "mermaid")]
        {
            match crate::mermaid::render_mermaid(code) {
                Ok(svg) => {
                    self.image_count += 1;
                    let path = format!("/media/diagram{}.svg", self.image_count);
                    let width = (svg.width * 0.75).min(TEXT_WIDTH_PT);
                    self.files.push((path.clone(), svg.svg.into_bytes()));
                    self.out.push_str(&format!(
                        "#align(center, image({}, width: {width:.1}pt))\n\n",
                        string(&path)
                    ));
                    return;
                }
                Err(e) => self.warn(
                    "mermaid-not-rendered",
                    format!("a Mermaid diagram could not be rendered ({e}); its source is shown instead"),
                ),
            }
        }
        #[cfg(not(feature = "mermaid"))]
        self.warn(
            "mermaid-not-rendered",
            "a Mermaid diagram is shown as its source: this build has no `mermaid` feature"
                .to_string(),
        );
        self.out.push_str(&format!(
            "#raw(block: true, lang: \"mermaid\", {})\n\n",
            string(code.trim_end())
        ));
    }

    /// Load an image once: (virtual path, natural width pt, natural height pt).
    fn image_file(&mut self, src: &str) -> Option<(String, f64, f64)> {
        if let Some(known) = self.image_paths.get(src) {
            return known.clone();
        }
        let loaded = match self.resolver.load_image(src) {
            Ok(img) => {
                let ext = match img.info.format {
                    ImageFormat::Png => Some("png"),
                    ImageFormat::Jpeg => Some("jpg"),
                    ImageFormat::Gif => Some("gif"),
                    ImageFormat::Webp => Some("webp"),
                    ImageFormat::Svg => Some("svg"),
                    _ => None,
                };
                match ext {
                    Some(ext) => {
                        self.image_count += 1;
                        let path = format!("/media/image{}.{ext}", self.image_count);
                        self.files.push((path.clone(), img.bytes));
                        Some((
                            path,
                            f64::from(img.info.width_px) * 0.75,
                            f64::from(img.info.height_px) * 0.75,
                        ))
                    }
                    None => {
                        self.warn(
                            "image-format-unsupported",
                            format!(
                                "image `{src}`: {:?} cannot be placed in a PDF; its alt text is shown",
                                img.info.format
                            ),
                        );
                        None
                    }
                }
            }
            Err(e) => {
                self.warn("image-not-loaded", format!("{e}; its alt text is shown"));
                None
            }
        };
        self.image_paths.insert(src.to_string(), loaded.clone());
        loaded
    }

    /// The `width:` / `height:` arguments for an image.
    fn size_args(attr: &Attr, natural_w: f64) -> String {
        let length = |v: &str| -> Option<String> {
            let v = v.trim();
            for unit in ["%", "in", "cm", "mm", "pt", "em"] {
                if let Some(n) = v.strip_suffix(unit) {
                    if n.trim().parse::<f64>().is_ok() {
                        return Some(format!("{}{unit}", n.trim()));
                    }
                }
            }
            let px = v.strip_suffix("px").unwrap_or(v);
            px.trim()
                .parse::<f64>()
                .ok()
                .map(|n| format!("{:.1}pt", (n * 0.75).min(TEXT_WIDTH_PT)))
        };
        if let Some(w) = attr.get("width").and_then(length) {
            return format!("width: {w}");
        }
        if let Some(h) = attr.get("height").and_then(length) {
            return format!("height: {h}");
        }
        format!("width: {:.1}pt", natural_w.min(TEXT_WIDTH_PT))
    }

    fn figure(&mut self, img: &Image) {
        let alt = img.alt_text();
        let Some((path, w, _)) = self.image_file(&img.src) else {
            if !alt.is_empty() {
                self.out.push_str(&format!("#emph[{}]\n\n", escape(&alt)));
            }
            return;
        };
        let size = Self::size_args(&img.attr, w);
        let image = format!("image({}, {size}, alt: {})", string(&path), string(&alt));
        let label = img
            .attr
            .id
            .as_deref()
            .filter(|i| valid_label(i))
            .map(|i| format!(" <{i}>"))
            .unwrap_or_default();
        if img.alt.is_empty() {
            self.out
                .push_str(&format!("#align(center, {image}){label}\n\n"));
        } else {
            let caption = self.inlines_one_line(&img.alt);
            self.out.push_str(&format!(
                "#figure({image}, caption: [{caption}]){label}\n\n"
            ));
        }
    }

    fn inlines_one_line(&mut self, inlines: &[Inline]) -> String {
        self.inlines(inlines)
            .replace("\\\n", " ")
            .replace('\n', " ")
    }

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut s = String::new();
        for i in inlines {
            self.inline(i, &mut s);
        }
        s
    }

    fn inline(&mut self, i: &Inline, s: &mut String) {
        match i {
            Inline::Text(t) => s.push_str(&escape(t)),
            // Every embedded call ends with `;` (swallowed by Typst), so a `.`, `(` or `[`
            // the author wrote next cannot continue the call.
            Inline::Emph(x) => s.push_str(&format!("#emph[{}];", self.inlines(x))),
            Inline::Strong(x) => s.push_str(&format!("#strong[{}];", self.inlines(x))),
            Inline::Strikeout(x) => s.push_str(&format!("#strike[{}];", self.inlines(x))),
            Inline::Superscript(x) => s.push_str(&format!("#super[{}];", self.inlines(x))),
            Inline::Subscript(x) => s.push_str(&format!("#sub[{}];", self.inlines(x))),
            Inline::Code(c) => s.push_str(&format!("#raw({});", string(c))),
            Inline::Math { display, tex } => self.math(*display, tex, s),
            Inline::Link { text, url, .. } => {
                let body = self.inlines(text);
                if let Some(anchor) = url.strip_prefix('#') {
                    if self.labels.contains(anchor) {
                        s.push_str(&format!("#link(<{anchor}>)[{body}];"));
                    } else {
                        s.push_str(&body);
                    }
                } else if url.is_empty() {
                    s.push_str(&body);
                } else {
                    s.push_str(&format!("#link({})[{body}];", string(url)));
                }
            }
            Inline::Image(img) => match self.image_file(&img.src) {
                Some((path, w, _)) => {
                    let size =
                        if img.attr.get("width").is_some() || img.attr.get("height").is_some() {
                            Self::size_args(&img.attr, w)
                        } else {
                            // An inline image keeps its size, up to the line height.
                            format!("height: {:.1}pt", w.clamp(1.0, 11.0))
                        };
                    s.push_str(&format!(
                        "#box(image({}, {size}, alt: {}));",
                        string(&path),
                        string(&img.alt_text())
                    ));
                }
                None => s.push_str(&escape(&img.alt_text())),
            },
            Inline::Cite(cites) => s.push_str(&escape(&cite_text(cites))),
            Inline::FootnoteRef(label) => self.footnote_ref(label, s),
            Inline::InlineNote(content) => {
                let body = self.inlines(content);
                s.push_str(&format!("#footnote[{body}];"));
            }
            Inline::RawHtml(h) => {
                let lower = h.trim().to_ascii_lowercase();
                if lower.starts_with("<br") {
                    s.push_str(" \\\n");
                }
            }
            Inline::SoftBreak => s.push(' '),
            Inline::HardBreak => s.push_str(" \\\n"),
        }
    }

    fn footnote_ref(&mut self, label: &str, s: &mut String) {
        if let Some(typst_label) = self.note_labels.get(label) {
            s.push_str(&format!("#footnote(<{typst_label}>);"));
            return;
        }
        let Some(content) = self.notes.get(label).cloned() else {
            self.warn(
                "footnote-missing",
                format!("footnote `[^{label}]` has no definition"),
            );
            s.push_str(&escape(&format!("[^{label}]")));
            return;
        };
        let typst_label = format!("md2star-fn-{}", self.note_labels.len() + 1);
        self.note_labels
            .insert(label.to_string(), typst_label.clone());
        let body = self.nested(&content);
        s.push_str(&format!("#footnote[{}] <{typst_label}>", body.trim()));
    }

    fn math(&mut self, display: bool, tex: &str, s: &mut String) {
        self.has_math = true;
        if self.math_mode == MathMode::Source {
            let code = format!("#raw({});", string(tex.trim()));
            if display {
                s.push_str(&format!("\n#align(center)[{code}]\n"));
            } else {
                s.push_str(&code);
            }
            return;
        }
        let (typst, unknown) = math::to_typst(tex);
        for u in unknown {
            self.warn(
                "math-unsupported",
                format!("`{u}` has no Typst equivalent here; it is shown as text"),
            );
        }
        if display {
            s.push_str(&format!("$ {typst} $"));
        } else if typst.is_empty() {
        } else {
            s.push_str(&format!("${typst}$"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_text_is_escaped() {
        assert_eq!(
            escape("a*b_c #d $e @f [g] <h> ~ // x"),
            "a\\*b\\_c \\#d \\$e \\@f \\[g\\] \\<h\\> \\~ \\// x"
        );
        assert_eq!(guard_start("= x".into()), "\\= x");
        assert_eq!(guard_start("12. x".into()), "12\\. x");
        assert_eq!(guard_start("-5 °C".into()), "\\-5 °C");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn languages_split_into_lang_and_region() {
        assert_eq!(lang_region("fr-FR"), (Some("fr".into()), Some("FR".into())));
        assert_eq!(lang_region("en"), (Some("en".into()), None));
        assert_eq!(lang_region("x"), (None, None));
    }
}
