//! DOCX writer: [`Document`] → `.docx` bytes, in process and in pure Rust.
//!
//! [`render`] writes WordprocessingML directly, with the vocabulary Pandoc's DOCX writer
//! uses, so that `md2star-rs docx` and `md2star docx` (Pandoc underneath) produce the
//! same kind of document, styled by the same reference templates:
//!
//! - **Title block**: the front matter's title, subtitle, authors, date and abstract in
//!   the `Title`, `Subtitle`, `Author`, `Date`, `AbstractTitle`/`Abstract` styles, also
//!   written to `docProps/core.xml`. Without a front-matter title, md2star's rule: the
//!   first level-1 heading becomes the title, and authors and date follow it in the
//!   `Subtitle` style.
//! - **Table of contents** (`--toc`, `toc: true`): a Word `TOC \o "1-3" \h \z \u` field
//!   marked dirty, so Word rebuilds it (with page numbers) on opening; its result is
//!   pre-filled with linked entries, so a reader that does not update fields (LibreOffice,
//!   a phone's preview) still shows one.
//! - **Text**: `FirstParagraph`/`BodyText`, bold, italic, strikeout, superscript,
//!   subscript, inline code (`VerbatimChar`), code blocks (`SourceCode`), block quotes
//!   (`BlockText`), page breaks for `---` (md2star's rule), native list numbering, task
//!   boxes `☐`/`☒`, Word footnotes for `[^n]` and `^[…]`.
//! - **Links**: external ones are real hyperlinks (a relationship and the `Hyperlink`
//!   style); `#id` links jump to the bookmark every heading, figure, table and div with
//!   an id carries.
//! - **Images** (`images.rs`): embedded, sized by [`crate::resources::fit_emu`], with
//!   their alt text; a figure is a centred `CaptionedFigure` paragraph and an
//!   `ImageCaption`.
//! - **Tables**: column alignments, a header row repeated across pages, md2star's
//!   `MyTable` / `MyTableSmall` styles (the small one from six columns or 800
//!   characters), the caption before the table in `TableCaption`.
//! - **Equations** ([`math`]): native OMML.
//! - **Citations** ([`citations`], [`crate::bib`]): rendered, with the reference list in
//!   the `Bibliography` style.
//! - **`--reference-doc`**: see `package.rs`. Every style used is taken from the
//!   reference (by id, else by name), or added to it when it lacks one.
//!
//! # Determinism
//!
//! Every id (relationships, footnotes, bookmarks, drawings, numbering) comes from a
//! per-document counter, the zip entries are written in a fixed order with a fixed
//! timestamp, and no date is stamped into the properties: the same Markdown and options
//! give the same bytes on every run.
//!
//! # Files this module owns
//!
//! `src/docx/` (this writer, `math.rs`, `citations.rs`, `images.rs`, `package.rs`,
//! `styles.rs`, `xml.rs`), the whole of `src/bib/`, `src/cli/docx.rs`,
//! and the `tests/docx_*.rs` integration tests. [`math::latex_to_omml`] is also the
//! LaTeX → OMML converter the PPTX writer calls.

pub mod citations;
mod images;
pub mod math;
mod package;
mod styles;
mod xml;

use std::collections::{HashMap, HashSet};

use crate::ast::{
    cite_text, plain_text, Align, Attr, Block, Document, Image, Inline, ListItem, Warning,
};
use crate::error::Result;
use crate::options::{Options, Rendered};
use crate::resources::{fit_emu, is_remote, Resolver};

use package::{Base, Output, RelSet};
use styles::StyleSheet;
use xml::{escape_attr, escape_text};

/// EMU per twip (1/20 point).
const EMU_PER_TWIP: u64 = 635;
/// Word supports nine list levels (0..=8).
const MAX_LEVEL: usize = 8;

/// Convert a parsed document to `.docx` bytes.
///
/// The entry point every caller (the library API, the CLI) goes through.
///
/// # Errors
///
/// [`crate::Error::Template`] when `options.reference_doc` is not a readable `.docx`;
/// [`crate::Error::Docx`] when packing fails. Everything else (a missing image or
/// bibliography, an unknown citation key or LaTeX command) is a warning.
pub fn render(doc: &Document, options: &Options) -> Result<Rendered> {
    let mut warnings = doc.warnings.clone();
    let (doc, more) = citations::apply(doc, options)?;
    warnings.extend(more);
    let lang = options.lang.clone().or_else(|| doc.meta.lang.clone());
    let base = match &options.reference_doc {
        Some(bytes) => Base::from_reference(bytes, lang.as_deref())?,
        // No reference: md2star's own `template.docx`, compiled in, as `md2star docx`
        // uses it by default.
        None => Base::from_reference(crate::templates::DEFAULT_DOCX, lang.as_deref())?,
    };
    let mut writer = Writer::new(&base, options, &doc, lang.clone());
    writer.document(&doc);
    warnings.append(&mut writer.warnings);
    let output = writer.finish(&base, &doc, lang.as_deref());
    Ok(Rendered {
        bytes: package::pack(base, output)?,
        warnings,
    })
}

/// Write `blocks` (a body with no front matter) to `.docx` bytes with the default
/// options: md2star's bundled template, no bibliography.
///
/// # Errors
///
/// See [`render`].
///
/// # Examples
///
/// ```
/// use md2star_rs::{docx, reader};
/// let bytes = docx::build(&reader::parse("# Title\n\nBody.")).unwrap();
/// assert_eq!(&bytes[..2], b"PK");
/// ```
pub fn build(blocks: &[Block]) -> Result<Vec<u8>> {
    render(&body_only(blocks), &Options::default()).map(|r| r.bytes)
}

/// Write `blocks` to `.docx` bytes styled by the reference document `reference` (the
/// bytes of a `.docx` or `.dotx`): its styles, theme, numbering, headers, footers and
/// page setup are kept, its body dropped.
///
/// # Errors
///
/// [`crate::Error::Template`] when `reference` is not a readable Word document.
pub fn build_with_reference(blocks: &[Block], reference: &[u8]) -> Result<Vec<u8>> {
    let options = Options {
        reference_doc: Some(reference.to_vec()),
        ..Options::default()
    };
    render(&body_only(blocks), &options).map(|r| r.bytes)
}

fn body_only(blocks: &[Block]) -> Document {
    Document {
        meta: crate::ast::Metadata::default(),
        blocks: blocks.to_vec(),
        warnings: Vec::new(),
    }
}

/// Character formatting in effect.
#[derive(Debug, Clone, Copy, Default)]
struct Fmt {
    bold: bool,
    italic: bool,
    strike: bool,
    sup: bool,
    sub: bool,
    code: bool,
    link: bool,
}

/// The writer's state for one document.
struct Writer<'a> {
    options: &'a Options,
    lang: Option<String>,
    styles: StyleSheet,
    resolver: Resolver,
    doc_rels: RelSet,
    foot_rels: RelSet,
    media: images::Media,
    /// The body being written (swapped out while a footnote is written).
    out: String,
    footnotes: Vec<String>,
    next_footnote: i64,
    separators: (i64, i64),
    footnote_defs: HashMap<String, Vec<Block>>,
    in_note: bool,
    bookmarks: HashSet<String>,
    next_bookmark: usize,
    next_drawing: usize,
    abstract_nums: String,
    nums: String,
    next_abstract: usize,
    next_num: usize,
    bullet_num: usize,
    decimal_abstract: usize,
    /// The next body paragraph opens a section (after a heading): `FirstParagraph`.
    first_para: bool,
    /// The previous block was a code block (two in a row get an empty paragraph
    /// between them, so a reader does not take them for one).
    after_code: bool,
    /// A paragraph style imposed by the context (quote, list, footnote, div).
    para_style: Option<String>,
    /// Content placed at the start of the next paragraph (a footnote's number).
    para_prefix: String,
    /// The text area for images, in EMU (the column inside a table).
    box_width: u64,
    box_height: u64,
    text_width_twips: u64,
    warnings: Vec<Warning>,
}

impl<'a> Writer<'a> {
    fn new(base: &Base, options: &'a Options, doc: &Document, lang: Option<String>) -> Self {
        let (max_abstract, max_num) = base
            .numbering
            .as_deref()
            .map(package::numbering_max)
            .unwrap_or((0, 0));
        let mut footnote_defs = HashMap::new();
        collect_footnote_defs(&doc.blocks, &mut footnote_defs);
        let mut w = Writer {
            options,
            lang,
            styles: StyleSheet::read(&base.styles),
            resolver: Resolver::from_options(options),
            doc_rels: RelSet::new(base.doc_rels.clone()),
            foot_rels: RelSet::new(Vec::new()),
            media: images::Media::default(),
            out: String::new(),
            footnotes: Vec::new(),
            next_footnote: base.separator_ids.0.max(base.separator_ids.1).max(0) + 1,
            separators: base.separator_ids,
            footnote_defs,
            in_note: false,
            bookmarks: HashSet::new(),
            next_bookmark: 1,
            next_drawing: 1,
            abstract_nums: String::new(),
            nums: String::new(),
            next_abstract: max_abstract + 1,
            next_num: max_num + 1,
            bullet_num: 0,
            decimal_abstract: 0,
            first_para: true,
            after_code: false,
            para_style: None,
            para_prefix: String::new(),
            box_width: base.page.text_width * EMU_PER_TWIP,
            box_height: base.page.text_height * EMU_PER_TWIP,
            text_width_twips: base.page.text_width,
            warnings: Vec::new(),
        };
        // The two list definitions every list shares.
        let bullet_abstract = w.alloc_abstract();
        w.abstract_nums
            .push_str(&list_definition(bullet_abstract, false, 1));
        w.decimal_abstract = w.alloc_abstract();
        w.abstract_nums
            .push_str(&list_definition(w.decimal_abstract, true, 1));
        w.bullet_num = w.alloc_num(bullet_abstract);
        w
    }

    fn warn(&mut self, code: &'static str, message: impl Into<String>) {
        self.warnings.push(Warning::new(code, message));
    }

    fn alloc_abstract(&mut self) -> usize {
        let id = self.next_abstract;
        self.next_abstract += 1;
        id
    }

    fn alloc_num(&mut self, abstract_id: usize) -> usize {
        let id = self.next_num;
        self.next_num += 1;
        self.nums.push_str(&format!(
            "<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{abstract_id}\"/></w:num>"
        ));
        id
    }

    /// The relationships of the part being written.
    fn rels(&mut self) -> &mut RelSet {
        if self.in_note {
            &mut self.foot_rels
        } else {
            &mut self.doc_rels
        }
    }

    fn style(&mut self, id: &'static str) -> String {
        self.styles.id(id)
    }

    // ---- The document ----

    fn document(&mut self, doc: &Document) {
        let promoted = self.title_block(doc);
        if self.options.toc || doc.meta.toc == Some(true) {
            self.toc(doc, promoted);
        }
        self.first_para = true;
        for (i, block) in doc.blocks.iter().enumerate() {
            if Some(i) == promoted {
                continue;
            }
            self.block(block, 0);
        }
    }

    /// The title block; returns the index of the heading promoted to title, if any.
    fn title_block(&mut self, doc: &Document) -> Option<usize> {
        let meta = &doc.meta;
        let authors: Vec<String> = match &self.options.author {
            Some(a) => a
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            None => meta.authors.clone(),
        };
        let date = self.options.date.clone().or_else(|| meta.date.clone());
        let (title, promoted): (Option<Vec<Inline>>, Option<usize>) = match &meta.title {
            Some(t) if !t.trim().is_empty() => (Some(inline_markdown(t)), None),
            _ => match doc.blocks.iter().position(is_title_candidate) {
                Some(i) => match &doc.blocks[i] {
                    Block::Heading { content, .. } => (Some(content.clone()), Some(i)),
                    _ => (None, None),
                },
                None => (None, None),
            },
        };
        if let Some(title) = &title {
            let style = self.style("Title");
            let content = self.inlines(title, Fmt::default());
            self.push_p(&style, "", &content);
        }
        if let Some(subtitle) = meta.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
            let style = self.style("Subtitle");
            let content = self.inlines(&inline_markdown(subtitle), Fmt::default());
            self.push_p(&style, "", &content);
        }
        if promoted.is_some() {
            // md2star: authors on one line, the date on the next, both as subtitle.
            let style = self.style("Subtitle");
            if !authors.is_empty() {
                let line = join_authors(&authors, self.lang.as_deref());
                let content = self.text_run(&line, Fmt::default());
                self.push_p(&style, "", &content);
            }
            if let Some(date) = date.as_deref().filter(|d| !d.trim().is_empty()) {
                let content = self.inlines(&inline_markdown(date), Fmt::default());
                self.push_p(&style, "", &content);
            }
        } else {
            for author in &authors {
                let style = self.style("Author");
                let content = self.inlines(&inline_markdown(author), Fmt::default());
                self.push_p(&style, "", &content);
            }
            if let Some(date) = date.as_deref().filter(|d| !d.trim().is_empty()) {
                let style = self.style("Date");
                let content = self.inlines(&inline_markdown(date), Fmt::default());
                self.push_p(&style, "", &content);
            }
        }
        if let Some(abstract_text) = meta
            .abstract_text
            .as_deref()
            .filter(|a| !a.trim().is_empty())
        {
            let style = self.style("AbstractTitle");
            let heading = match meta.raw.get("abstract-title").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => localised(self.lang.as_deref(), "Abstract", "Résumé").to_string(),
            };
            let content = self.text_run(&heading, Fmt::default());
            self.push_p(&style, "", &content);
            let style = self.style("Abstract");
            let saved = self.para_style.replace(style);
            for block in crate::reader::parse(abstract_text) {
                self.block(&block, 0);
            }
            self.para_style = saved;
        }
        promoted
    }

    /// The table of contents: a dirty `TOC` field whose result is pre-filled.
    fn toc(&mut self, doc: &Document, promoted: Option<usize>) {
        let depth = doc
            .meta
            .raw
            .get("toc-depth")
            .and_then(|v| v.as_str())
            .and_then(|d| d.trim().parse::<u8>().ok())
            .unwrap_or(3)
            .clamp(1, 9);
        let title = doc
            .meta
            .raw
            .get("toc-title")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                localised(
                    self.lang.as_deref(),
                    "Table of Contents",
                    "Table des matières",
                )
                .into()
            });
        let mut entries = Vec::new();
        for (i, block) in doc.blocks.iter().enumerate() {
            if Some(i) != promoted {
                collect_headings(block, depth, &mut entries);
            }
        }
        let heading_style = self.style("TOCHeading");
        let mut s = String::from(
            "<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Table of Contents\"/>\
             <w:docPartUnique/></w:docPartObj></w:sdtPr><w:sdtContent>",
        );
        s.push_str(&p_xml(Some(&heading_style), "", &plain_run(&title)));
        let begin = format!(
            "<w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/></w:r>\
             <w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-{depth}\" \\h \\z \\u </w:instrText></w:r>\
             <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>"
        );
        let end = "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>";
        if entries.is_empty() {
            s.push_str(&p_xml(None, "", &format!("{begin}{end}")));
        } else {
            let last = entries.len() - 1;
            for (i, (level, id, text)) in entries.iter().enumerate() {
                let style = match level {
                    1 => self.style("TOC1"),
                    2 => self.style("TOC2"),
                    _ => self.style("TOC3"),
                };
                let mut content = String::new();
                if i == 0 {
                    content.push_str(&begin);
                }
                content.push_str(&format!(
                    "<w:hyperlink w:anchor=\"{}\" w:history=\"1\">{}</w:hyperlink>",
                    escape_attr(id),
                    plain_run(text)
                ));
                if i == last {
                    content.push_str(end);
                }
                s.push_str(&p_xml(Some(&style), "", &content));
            }
        }
        s.push_str("</w:sdtContent></w:sdt>");
        self.out.push_str(&s);
    }

    // ---- Blocks ----

    fn push_p(&mut self, style: &str, extra_ppr: &str, content: &str) {
        let prefix = std::mem::take(&mut self.para_prefix);
        let content = format!("{prefix}{content}");
        self.out.push_str(&p_xml(
            Some(style).filter(|s| !s.is_empty()),
            extra_ppr,
            &content,
        ));
    }

    /// The style of an ordinary paragraph here.
    fn body_style(&mut self) -> String {
        if let Some(style) = &self.para_style {
            return style.clone();
        }
        if std::mem::take(&mut self.first_para) {
            self.style("FirstParagraph")
        } else {
            self.style("BodyText")
        }
    }

    fn block(&mut self, block: &Block, depth: usize) {
        let after_code = std::mem::take(&mut self.after_code);
        match block {
            Block::Heading {
                level,
                content,
                attr,
            } => {
                let style = match level {
                    1 => self.style("Heading1"),
                    2 => self.style("Heading2"),
                    3 => self.style("Heading3"),
                    4 => self.style("Heading4"),
                    5 => self.style("Heading5"),
                    _ => self.style("Heading6"),
                };
                let inner = self.inlines(content, Fmt::default());
                let content = self.bookmarked(attr.id.as_deref(), &inner);
                self.push_p(&style, "", &content);
                self.first_para = true;
            }
            Block::Paragraph(inlines) => {
                let style = self.body_style();
                let content = self.inlines(inlines, Fmt::default());
                self.push_p(&style, "", &content);
            }
            Block::Figure(image) => self.figure(image),
            Block::CodeBlock { code, attr, .. } => {
                if after_code {
                    let style = self.body_style();
                    self.push_p(&style, "", "");
                }
                self.code_block(code, attr);
                self.after_code = true;
            }
            Block::Mermaid { code, attr } => {
                match crate::mermaid_picture(code, &mut self.warnings) {
                    Some(Block::Paragraph(inlines)) => {
                        if let Some(Inline::Image(image)) = inlines.into_iter().next() {
                            // A figure without a caption (the alt text stays the
                            // picture's description), carrying the block's id.
                            let mut attr = attr.clone();
                            attr.kv.push(("fig-alt".into(), image.alt_text()));
                            self.figure(&Image {
                                alt: Vec::new(),
                                attr,
                                ..image
                            });
                        }
                    }
                    Some(other) => self.block(&other, depth),
                    None => self.code_block(code, attr),
                }
            }
            Block::BlockQuote(inner) => {
                let style = self.style("BlockText");
                let saved = self.para_style.replace(style);
                for child in inner {
                    self.block(child, depth);
                }
                self.para_style = saved;
                self.first_para = false;
            }
            Block::List {
                ordered,
                start,
                items,
            } => self.list(*ordered, *start, items, depth),
            Block::Table {
                headers,
                rows,
                aligns,
                caption,
                attr,
            } => self.table(headers, rows, aligns, caption, attr),
            Block::Div { attr, content } => self.div(attr, content, depth),
            // Speaker notes belong to slides; raw HTML has no Word equivalent.
            Block::Notes(_) | Block::RawHtml(_) => {}
            // md2star: `---` is a page break in a document.
            Block::ThematicBreak => {
                if !self.in_note {
                    self.out
                        .push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
                }
            }
            // Definitions are written where they are referenced.
            Block::FootnoteDef { .. } => {}
        }
    }

    /// `<w:bookmarkStart/>content<w:bookmarkEnd/>` when `id` is a new bookmark name.
    fn bookmarked(&mut self, id: Option<&str>, content: &str) -> String {
        match id.filter(|id| !id.is_empty()) {
            Some(name) if !self.in_note && self.bookmarks.insert(name.to_string()) => {
                let n = self.next_bookmark;
                self.next_bookmark += 1;
                format!(
                    "<w:bookmarkStart w:id=\"{n}\" w:name=\"{}\"/>{content}<w:bookmarkEnd w:id=\"{n}\"/>",
                    escape_attr(name)
                )
            }
            _ => content.to_string(),
        }
    }

    fn code_block(&mut self, code: &str, attr: &Attr) {
        let style = self.style("SourceCode");
        let char_style = self.style("VerbatimChar");
        let lines: Vec<String> = code
            .trim_end_matches('\n')
            .split('\n')
            .map(|line| {
                run_xml(
                    line,
                    &format!("<w:rStyle w:val=\"{}\"/>", escape_attr(&char_style)),
                )
            })
            .collect();
        let content = lines.join("<w:r><w:br/></w:r>");
        let content = self.bookmarked(attr.id.as_deref(), &content);
        self.push_p(&style, "", &content);
        self.first_para = false;
    }

    fn figure(&mut self, image: &Image) {
        let has_caption = !image.alt.is_empty();
        let style = if has_caption {
            self.style("CaptionedFigure")
        } else {
            self.style("Figure")
        };
        let descr = image
            .attr
            .get("fig-alt")
            .map(str::to_string)
            .unwrap_or_else(|| image.alt_text());
        let drawing = self.image(image, &descr);
        let content = self.bookmarked(image.attr.id.as_deref(), &drawing);
        // Centred, as md2star's post-processing centres standalone images.
        self.push_p(&style, "<w:jc w:val=\"center\"/>", &content);
        if has_caption {
            let style = self.style("ImageCaption");
            let caption = self.inlines(&image.alt, Fmt::default());
            self.push_p(&style, "", &caption);
        }
        self.first_para = false;
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[ListItem], depth: usize) {
        let num = if ordered {
            let start = usize::try_from(start).unwrap_or(1).max(1);
            // OOXML puts the first number on the abstract definition: a list that does not
            // start at 1 gets a definition of its own.
            let abstract_id = if start == 1 {
                self.decimal_abstract
            } else {
                let id = self.alloc_abstract();
                self.abstract_nums
                    .push_str(&list_definition(id, true, start));
                id
            };
            self.alloc_num(abstract_id)
        } else {
            self.bullet_num
        };
        let level = depth.min(MAX_LEVEL);
        let compact = self.style("Compact");
        let numbered =
            format!("<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>");
        let continued = format!("<w:ind w:left=\"{}\"/>", 720 * (level + 1));
        let saved = self.para_style.replace(compact.clone());
        for item in items {
            let mut first = true;
            let task = item.task.map(|done| if done { "☒ " } else { "☐ " });
            for child in &item.content {
                match child {
                    Block::Paragraph(inlines) => {
                        let mut content = String::new();
                        if first {
                            if let Some(task) = task {
                                content.push_str(&plain_run(task));
                            }
                        }
                        content.push_str(&self.inlines(inlines, Fmt::default()));
                        let ppr = if first { &numbered } else { &continued };
                        self.push_p(&compact, ppr, &content);
                    }
                    Block::List {
                        ordered,
                        start,
                        items,
                    } => {
                        if first {
                            self.push_p(&compact, &numbered, "");
                        }
                        self.list(*ordered, *start, items, depth + 1);
                    }
                    other => {
                        if first {
                            let content = task.map(plain_run).unwrap_or_default();
                            self.push_p(&compact, &numbered, &content);
                        }
                        self.block(other, depth + 1);
                    }
                }
                first = false;
            }
            if first {
                // An empty item still shows its bullet.
                self.push_p(&compact, &numbered, "");
            }
        }
        self.para_style = saved;
        self.first_para = false;
    }

    fn table(
        &mut self,
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        aligns: &[Align],
        caption: &[Inline],
        attr: &Attr,
    ) {
        let ncols = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return;
        }
        if !caption.is_empty() {
            let style = self.style("TableCaption");
            let content = self.inlines(caption, Fmt::default());
            let content = self.bookmarked(attr.id.as_deref(), &content);
            self.push_p(&style, "", &content);
        }
        // md2star's rule: the small style for wide or wordy tables.
        let chars: usize = headers
            .iter()
            .chain(rows.iter().flatten())
            .map(|c| plain_text(c).chars().count())
            .sum();
        let table_style = if ncols >= 6 || chars > 800 {
            self.style("MyTableSmall")
        } else {
            self.style("MyTable")
        };
        let col_twips = self.text_width_twips / ncols as u64;
        let mut s = format!(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"{}\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/>\
             <w:tblLook w:val=\"0020\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" \
             w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"0\"/>",
            escape_attr(&table_style)
        );
        if !caption.is_empty() {
            s.push_str(&format!(
                "<w:tblCaption w:val=\"{}\"/>",
                escape_attr(&plain_text(caption))
            ));
        }
        s.push_str("</w:tblPr><w:tblGrid>");
        for _ in 0..ncols {
            s.push_str(&format!("<w:gridCol w:w=\"{col_twips}\"/>"));
        }
        s.push_str("</w:tblGrid>");
        let compact = self.style("Compact");
        let (box_width, box_height) = (self.box_width, self.box_height);
        self.box_width = col_twips * EMU_PER_TWIP;
        let saved = self.para_style.replace(compact.clone());
        let has_header = headers.iter().any(|h| !h.is_empty());
        let write_row = |w: &mut Self, cells: &[Vec<Inline>], header: bool| {
            let mut row = String::from("<w:tr>");
            if header {
                row.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for i in 0..ncols {
                let jc = match aligns.get(i).copied().unwrap_or_default() {
                    Align::Left => "<w:jc w:val=\"left\"/>",
                    Align::Center => "<w:jc w:val=\"center\"/>",
                    Align::Right => "<w:jc w:val=\"right\"/>",
                    Align::Default => "",
                };
                let fmt = Fmt {
                    bold: header,
                    ..Fmt::default()
                };
                let content = cells.get(i).map(|c| w.inlines(c, fmt)).unwrap_or_default();
                row.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{col_twips}\" w:type=\"dxa\"/></w:tcPr>{}</w:tc>",
                    p_xml(Some(&compact), jc, &content)
                ));
            }
            row.push_str("</w:tr>");
            row
        };
        if has_header {
            let row = write_row(self, headers, true);
            s.push_str(&row);
        }
        for cells in rows {
            let row = write_row(self, cells, false);
            s.push_str(&row);
        }
        s.push_str("</w:tbl>");
        self.para_style = saved;
        self.box_width = box_width;
        self.box_height = box_height;
        self.out.push_str(&s);
        self.first_para = false;
    }

    fn div(&mut self, attr: &Attr, content: &[Block], depth: usize) {
        let custom = attr.get("custom-style").map(str::to_string);
        let saved = match custom {
            Some(name) => {
                let style = self.styles.custom(&name);
                Some(self.para_style.replace(style))
            }
            None => None,
        };
        let bookmark = attr
            .id
            .as_deref()
            .filter(|id| !id.is_empty() && !self.in_note && !self.bookmarks.contains(*id))
            .map(str::to_string);
        let n = self.next_bookmark;
        if let Some(name) = &bookmark {
            self.bookmarks.insert(name.clone());
            self.next_bookmark += 1;
            self.out.push_str(&format!(
                "<w:bookmarkStart w:id=\"{n}\" w:name=\"{}\"/>",
                escape_attr(name)
            ));
        }
        for child in content {
            self.block(child, depth);
        }
        if bookmark.is_some() {
            self.out.push_str(&format!("<w:bookmarkEnd w:id=\"{n}\"/>"));
        }
        if let Some(saved) = saved {
            self.para_style = saved;
        }
    }

    // ---- Inlines ----

    fn text_run(&mut self, text: &str, fmt: Fmt) -> String {
        let mut rpr = String::new();
        if fmt.code {
            let s = self.style("VerbatimChar");
            rpr.push_str(&format!("<w:rStyle w:val=\"{}\"/>", escape_attr(&s)));
        } else if fmt.link {
            let s = self.style("Hyperlink");
            rpr.push_str(&format!("<w:rStyle w:val=\"{}\"/>", escape_attr(&s)));
        }
        if fmt.bold {
            rpr.push_str("<w:b/><w:bCs/>");
        }
        if fmt.italic {
            rpr.push_str("<w:i/><w:iCs/>");
        }
        if fmt.strike {
            rpr.push_str("<w:strike/>");
        }
        if fmt.sup {
            rpr.push_str("<w:vertAlign w:val=\"superscript\"/>");
        } else if fmt.sub {
            rpr.push_str("<w:vertAlign w:val=\"subscript\"/>");
        }
        run_xml(text, &rpr)
    }

    fn inlines(&mut self, inlines: &[Inline], fmt: Fmt) -> String {
        let mut s = String::new();
        for inline in inlines {
            match inline {
                Inline::Text(t) => s.push_str(&self.text_run(t, fmt)),
                Inline::Emph(inner) => s.push_str(&self.inlines(
                    inner,
                    Fmt {
                        italic: true,
                        ..fmt
                    },
                )),
                Inline::Strong(inner) => {
                    s.push_str(&self.inlines(inner, Fmt { bold: true, ..fmt }))
                }
                Inline::Strikeout(inner) => s.push_str(&self.inlines(
                    inner,
                    Fmt {
                        strike: true,
                        ..fmt
                    },
                )),
                Inline::Superscript(inner) => s.push_str(&self.inlines(
                    inner,
                    Fmt {
                        sup: true,
                        sub: false,
                        ..fmt
                    },
                )),
                Inline::Subscript(inner) => s.push_str(&self.inlines(
                    inner,
                    Fmt {
                        sub: true,
                        sup: false,
                        ..fmt
                    },
                )),
                Inline::Code(t) => s.push_str(&self.text_run(t, Fmt { code: true, ..fmt })),
                Inline::Math { display, tex } => {
                    let out = math::convert(tex, *display);
                    self.warnings.extend(out.warnings);
                    s.push_str(&out.xml);
                }
                Inline::Link {
                    text, url, title, ..
                } => s.push_str(&self.link(text, url, title.as_deref(), fmt)),
                Inline::Image(image) => {
                    let descr = image.alt_text();
                    s.push_str(&self.image(image, &descr));
                }
                // Unresolved (no bibliography): as written, as Pandoc without citeproc.
                Inline::Cite(cites) => s.push_str(&self.text_run(&cite_text(cites), fmt)),
                Inline::FootnoteRef(label) => match self.footnote_defs.get(label).cloned() {
                    Some(content) if !self.in_note => s.push_str(&self.footnote(&content)),
                    _ => s.push_str(&self.text_run(&format!("[^{label}]"), fmt)),
                },
                Inline::InlineNote(content) => {
                    if self.in_note {
                        let text = format!("({})", plain_text(content));
                        s.push_str(&self.text_run(&text, fmt));
                    } else {
                        s.push_str(&self.footnote(&[Block::Paragraph(content.clone())]));
                    }
                }
                Inline::RawHtml(html) => {
                    let tag = html.trim().to_ascii_lowercase();
                    if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                        s.push_str("<w:r><w:br/></w:r>");
                    }
                }
                Inline::SoftBreak => s.push_str(&self.text_run(" ", fmt)),
                Inline::HardBreak => s.push_str("<w:r><w:br/></w:r>"),
            }
        }
        s
    }

    fn link(&mut self, text: &[Inline], url: &str, title: Option<&str>, fmt: Fmt) -> String {
        if fmt.link || url.is_empty() {
            return self.inlines(text, fmt);
        }
        let inner = self.inlines(text, Fmt { link: true, ..fmt });
        let tooltip = title
            .filter(|t| !t.is_empty())
            .map(|t| format!(" w:tooltip=\"{}\"", escape_attr(t)))
            .unwrap_or_default();
        if let Some(anchor) = url.strip_prefix('#') {
            return format!(
                "<w:hyperlink w:anchor=\"{}\"{tooltip} w:history=\"1\">{inner}</w:hyperlink>",
                escape_attr(anchor)
            );
        }
        let rid = self.rels().add(package::rel_type::HYPERLINK, url, true);
        format!("<w:hyperlink r:id=\"{rid}\"{tooltip} w:history=\"1\">{inner}</w:hyperlink>")
    }

    /// A Word footnote holding `blocks`; returns the reference run.
    fn footnote(&mut self, blocks: &[Block]) -> String {
        let id = self.next_footnote;
        self.next_footnote += 1;
        let note_style = self.style("FootnoteText");
        let ref_style = self.style("FootnoteReference");
        let saved_out = std::mem::take(&mut self.out);
        let saved_style = self.para_style.replace(note_style.clone());
        let saved_first = self.first_para;
        self.in_note = true;
        self.para_prefix = format!(
            "<w:r><w:rPr><w:rStyle w:val=\"{}\"/></w:rPr><w:footnoteRef/></w:r>{}",
            escape_attr(&ref_style),
            run_xml(" ", "")
        );
        for block in blocks {
            self.block(block, 0);
        }
        if !self.para_prefix.is_empty() {
            // Nothing was written: the note still needs its paragraph and number.
            self.push_p(&note_style, "", "");
        }
        self.in_note = false;
        self.para_style = saved_style;
        self.first_para = saved_first;
        let body = std::mem::replace(&mut self.out, saved_out);
        self.footnotes
            .push(format!("<w:footnote w:id=\"{id}\">{body}</w:footnote>"));
        format!(
            "<w:r><w:rPr><w:rStyle w:val=\"{}\"/></w:rPr><w:footnoteReference w:id=\"{id}\"/></w:r>",
            escape_attr(&ref_style)
        )
    }

    /// The run that shows `image`, or its alt text with a warning when it cannot be
    /// embedded.
    fn image(&mut self, image: &Image, descr: &str) -> String {
        let fallback = |w: &mut Self, why: String| {
            w.warn(
                "image-not-embedded",
                format!("image `{}` not embedded: {why}", short(&image.src)),
            );
            let text = if descr.is_empty() {
                image.src.clone()
            } else {
                descr.to_string()
            };
            w.text_run(
                &format!("[{text}]"),
                Fmt {
                    italic: true,
                    ..Fmt::default()
                },
            )
        };
        if is_remote(&image.src) && !self.resolver.allow_remote {
            return fallback(
                self,
                "remote images are off (pass --allow-remote-images)".into(),
            );
        }
        let loaded = match self.resolver.load_image(&image.src) {
            Ok(l) => l,
            Err(e) => return fallback(self, e.to_string()),
        };
        let prepared = match images::prepare(&loaded) {
            Ok(p) => p,
            Err(why) => return fallback(self, why),
        };
        if prepared.placeholder {
            self.warn(
                "svg-without-fallback",
                format!(
                    "SVG `{}` is embedded without a raster fallback (md2star-rs built without \
                     the `svg` feature): Word 2016+ shows it, older readers show an empty frame",
                    short(&image.src)
                ),
            );
        }
        let (cx, cy) = fit_emu(&loaded.info, &image.attr, self.box_width, self.box_height);
        let target = self.media.store(prepared.raster, prepared.ext);
        let rid = self.rels().add(package::rel_type::IMAGE, &target, false);
        let svg_rid = prepared.svg.map(|svg| {
            let target = self.media.store(svg, "svg");
            self.rels().add(package::rel_type::IMAGE, &target, false)
        });
        let id = self.next_drawing;
        self.next_drawing += 1;
        let name = if image.src.starts_with("data:") {
            "data URI"
        } else {
            image.src.as_str()
        };
        images::drawing_run(&images::Drawing {
            rid: &rid,
            svg_rid: svg_rid.as_deref(),
            cx,
            cy,
            id,
            descr,
            title: image.title.as_deref().unwrap_or(""),
            name,
        })
    }

    // ---- Packaging ----

    fn finish(self, base: &Base, doc: &Document, lang: Option<&str>) -> Output {
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NAMESPACES}><w:body>{}{}</w:body></w:document>",
            self.out, base.sect_pr
        );
        let (sep, cont) = self.separators;
        let mut footnotes = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {NAMESPACES}>\
             <w:footnote w:type=\"separator\" w:id=\"{sep}\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote>\
             <w:footnote w:type=\"continuationSeparator\" w:id=\"{cont}\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"
        );
        for note in &self.footnotes {
            footnotes.push_str(note);
        }
        footnotes.push_str("</w:footnotes>");
        let styles = self.styles.inject(&base.styles);
        let numbering =
            package::merge_numbering(base.numbering.as_deref(), &self.abstract_nums, &self.nums);
        let meta = &doc.meta;
        let authors: Vec<String> = match &self.options.author {
            Some(a) => a
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            None => meta.authors.clone(),
        };
        let title = meta.title.clone().or_else(|| {
            doc.blocks
                .iter()
                .find(|b| is_title_candidate(b))
                .and_then(|b| match b {
                    Block::Heading { content, .. } => Some(plain_text(content)),
                    _ => None,
                })
        });
        let core = package::core_properties(
            title.as_deref(),
            meta.subtitle.as_deref(),
            &authors,
            &meta.keywords,
            meta.abstract_text.as_deref(),
            lang,
        );
        Output {
            document,
            footnotes,
            doc_rels: self.doc_rels,
            foot_rels: self.foot_rels,
            media: self.media.into_parts(),
            styles,
            numbering,
            core,
        }
    }
}

/// The namespaces every part we write declares.
const NAMESPACES: &str =
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" \
xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\" \
xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\" \
mc:Ignorable=\"w14 w15 wp14\"";

/// `<w:p>` with a style, extra paragraph properties (already in schema order: `numPr`,
/// `ind`, `jc`) and content.
fn p_xml(style: Option<&str>, extra_ppr: &str, content: &str) -> String {
    let mut ppr = String::new();
    if let Some(style) = style {
        ppr.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_attr(style)));
    }
    ppr.push_str(extra_ppr);
    if ppr.is_empty() {
        format!("<w:p>{content}</w:p>")
    } else {
        format!("<w:p><w:pPr>{ppr}</w:pPr>{content}</w:p>")
    }
}

/// A run of text with run properties (tabs become `<w:tab/>`).
fn run_xml(text: &str, rpr: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let rpr = if rpr.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{rpr}</w:rPr>")
    };
    let mut body = String::new();
    for (i, piece) in text.split('\t').enumerate() {
        if i > 0 {
            body.push_str("<w:tab/>");
        }
        if !piece.is_empty() {
            body.push_str(&format!(
                "<w:t xml:space=\"preserve\">{}</w:t>",
                escape_text(piece)
            ));
        }
    }
    format!("<w:r>{rpr}{body}</w:r>")
}

fn plain_run(text: &str) -> String {
    run_xml(text, "")
}

/// A list definition: nine levels of bullets (`•`, `◦`, `▪`) or of decimals (each level
/// numbered on its own, `1.`), indented 0.5 in per level with a hanging marker.
fn list_definition(id: usize, ordered: bool, start: usize) -> String {
    let mut s = format!(
        "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
    );
    for level in 0..=MAX_LEVEL {
        let (fmt, text) = if ordered {
            ("decimal", format!("%{}.", level + 1))
        } else {
            (
                "bullet",
                match level % 3 {
                    0 => "•",
                    1 => "◦",
                    _ => "▪",
                }
                .to_string(),
            )
        };
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{fmt}\"/>\
             <w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/>\
             <w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            if level == 0 { start } else { 1 },
            720 * (level + 1)
        ));
    }
    s.push_str("</w:abstractNum>");
    s
}

/// Front-matter text read as Markdown inlines (`title: "A *bold* claim"`).
fn inline_markdown(text: &str) -> Vec<Inline> {
    match crate::reader::parse(text).into_iter().next() {
        Some(Block::Paragraph(inlines)) => inlines,
        _ => vec![Inline::Text(text.to_string())],
    }
}

/// A level-1 heading md2star may promote to the title (never the bibliography's).
fn is_title_candidate(block: &Block) -> bool {
    matches!(block, Block::Heading { level: 1, attr, .. } if !attr.has_class("bibliography-heading"))
}

/// md2star's author line: `A`, `A and B`, `A, B and C` (the conjunction in the
/// document's language).
fn join_authors(authors: &[String], lang: Option<&str>) -> String {
    let conj = match lang
        .and_then(|l| l.split(['-', '_']).next())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("fr") => "et",
        Some("es") => "y",
        Some("de") => "und",
        Some("it" | "pt") => "e",
        Some("nl") => "en",
        Some("ru") => "и",
        _ => "and",
    };
    match authors {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {conj} {last}", init.join(", ")),
    }
}

fn localised(lang: Option<&str>, en: &'static str, fr: &'static str) -> &'static str {
    if lang.is_some_and(|l| l.to_ascii_lowercase().starts_with("fr")) {
        fr
    } else {
        en
    }
}

fn collect_footnote_defs(blocks: &[Block], out: &mut HashMap<String, Vec<Block>>) {
    for block in blocks {
        match block {
            Block::FootnoteDef { label, content } => {
                out.entry(label.clone()).or_insert_with(|| content.clone());
            }
            Block::Div { content, .. } | Block::BlockQuote(content) => {
                collect_footnote_defs(content, out);
            }
            _ => {}
        }
    }
}

/// Headings for the table of contents: (level, id, text).
fn collect_headings(block: &Block, depth: u8, out: &mut Vec<(u8, String, String)>) {
    match block {
        Block::Heading {
            level,
            content,
            attr,
        } if *level <= depth => {
            if let Some(id) = attr.id.as_deref().filter(|id| !id.is_empty()) {
                out.push((*level, id.to_string(), plain_text(content)));
            }
        }
        Block::Div { content, .. } => {
            for child in content {
                collect_headings(child, depth, out);
            }
        }
        _ => {}
    }
}

/// A reference shortened for a message (`data:` URIs are long).
fn short(src: &str) -> String {
    if src.chars().count() > 60 {
        format!("{}…", src.chars().take(60).collect::<String>())
    } else {
        src.to_string()
    }
}
