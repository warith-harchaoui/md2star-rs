//! PPTX writer: [`Document`] → `.pptx` bytes, in pure Rust.
//!
//! md2star's `md2pptx`, in process: no Pandoc, no python-pptx, no LibreOffice. The deck
//! is built **on a template** — the bundled default (md2star's own `template.pptx`) or
//! `--reference-doc` / `--template` — keeping its masters, theme and layouts:
//!
//! 1. [`layout::segment`] cuts the document into slides (`#`/`##` and `---`, notes,
//!    cover from the front matter, `#` after the cover = section);
//! 2. [`template::inspect`] reads the template's layouts with every placeholder's box
//!    and text sizes resolved through layout and master;
//! 3. [`layout::choose_role`] and the deterministic selector place each slide on a
//!    layout — explicit `{layout="Name"}`, role classes, recognised forms, and a fit
//!    score measured with the theme fonts' real metrics ([`fonts`], [`fit`]); with
//!    `--min-pt N` nothing is set under N and a slide no layout holds is split into
//!    continuation slides, never shrunk; a [`judge::LayoutJudge`] (a language model,
//!    feature `llm`, or the host app's own) only breaks near-ties;
//! 4. the writer instantiates each layout's placeholders, adds tables (`a:tbl`), code
//!    panels, pictures with alt text and photo credits, real hyperlinks and notes
//!    slides, and packs the zip byte-deterministically.
//!
//! Footnotes become superscript numbers and a closing "Notes" slide, as Pandoc does;
//! citations resolve through [`crate::bib`] when it can, else print as written.

pub mod content;
pub mod fit;
pub mod fonts;
pub mod images;
pub mod judge;
pub mod layout;
pub mod linebreak;
pub mod template;
mod writer;
pub(crate) mod xml;

use std::collections::BTreeMap;

pub use writer::{short_credit, CREDIT_NAME, FIXED_TIMESTAMP};

use crate::ast::{plain_text, Block, Document, Inline, Metadata, Warning};
use crate::error::{Error, Result};
use crate::options::{Options, Rendered};
use crate::resources::Resolver;

/// The monospaced face of code: `MD2STAR_MONO_FONT`, else Consolas (md2star's default).
pub fn mono_font() -> String {
    std::env::var("MD2STAR_MONO_FONT")
        .ok()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "Consolas".into())
}

/// Convert a parsed document to `.pptx` bytes.
///
/// The entry point every caller goes through. With `options.smart_layout` and the `llm`
/// feature, near-ties between layouts are arbitrated by the OpenAI-compatible endpoint
/// the options (or `MD2STAR_LLM_ENDPOINT`) name; see [`render_with_judge`] to bring
/// another judge.
///
/// # Errors
///
/// [`crate::Error::Template`] when the reference template is not a usable `.pptx`,
/// [`crate::Error::Pptx`] when the deck cannot be packed.
pub fn render(doc: &Document, options: &Options) -> Result<Rendered> {
    let mut warnings = Vec::new();
    #[cfg(feature = "llm")]
    {
        if options.smart_layout {
            match crate::llm::Endpoint::from_options(options) {
                Some(endpoint) => {
                    let judge = crate::llm::HttpJudge::new(endpoint);
                    let cache = match judge::VerdictCache::default_dir() {
                        Some(dir) => judge::VerdictCache::on_disk(dir),
                        None => judge::VerdictCache::in_memory(),
                    };
                    let cached = judge::CachedJudge {
                        inner: &judge,
                        cache: &cache,
                    };
                    let mut out = render_with_judge(doc, options, Some(&cached))?;
                    out.warnings.extend(judge.take_warnings());
                    return Ok(out);
                }
                None => warnings.push(Warning::new(
                    "smart-layout-unavailable",
                    "--smart-layout: no endpoint (--llm-endpoint or MD2STAR_LLM_ENDPOINT); \
                     the deterministic layout choice is used",
                )),
            }
        }
    }
    #[cfg(not(feature = "llm"))]
    if options.smart_layout {
        warnings.push(Warning::new(
            "smart-layout-unavailable",
            "--smart-layout needs the `llm` feature; the deterministic layout choice is used",
        ));
    }
    let mut out = render_with_judge(doc, options, None)?;
    warnings.append(&mut out.warnings);
    out.warnings = warnings;
    Ok(out)
}

/// [`render`] with an explicit near-tie arbiter (`None`: fully deterministic).
///
/// # Errors
///
/// As [`render`].
pub fn render_with_judge(
    doc: &Document,
    options: &Options,
    judge: Option<&dyn judge::LayoutJudge>,
) -> Result<Rendered> {
    let mut warnings = doc.warnings.clone();
    let doc = apply_options(doc, options);
    let doc = apply_bibliography(&doc, options, &mut warnings);
    let doc = Document {
        blocks: render_diagrams(&doc.blocks, &mut warnings),
        ..doc
    };
    let template_bytes: &[u8] = match &options.reference_doc {
        Some(bytes) => bytes,
        None => crate::templates::DEFAULT_PPTX,
    };
    let package =
        template::Package::open(template_bytes).map_err(|e| Error::Template(e.to_string()))?;
    let info = template::template_info(&package)?;
    let mut typo = fit::Typo {
        min_pt: options.min_pt.filter(|m| *m > 0.0),
        mono_font: mono_font(),
        rank_pt: 0.0,
    };
    typo.rank_pt = rank_size(&info, &typo);

    // Images, loaded once.
    let resolver = Resolver::from_options(options);
    let mut all = Vec::new();
    content::images_in(&doc.blocks, &mut all);
    let mut images = BTreeMap::new();
    for img in all {
        if images.contains_key(&img.src) {
            continue;
        }
        match resolver.load_image(&img.src) {
            Ok(loaded) => {
                images.insert(img.src.clone(), loaded);
            }
            Err(e) => warnings.push(Warning::new(
                "image-not-loaded",
                format!("image `{}` was not embedded: {e}", img.src),
            )),
        }
    }

    // Footnotes: numbered in order of first reference; their text closes the deck.
    let (numbers, notes_slide) = footnotes(&doc);
    let mut slides = layout::segment(&doc)?;
    // Roles are decided over the authored slides: the closing stays the closing even
    // though the footnotes slide follows it.
    let total = slides.len();
    if let Some(mut s) = notes_slide {
        s.index = total + 1;
        slides.push(s);
    }
    let lang = doc.meta.lang.clone().unwrap_or_else(|| "en-US".into());
    let sample: String = slides
        .iter()
        .take(12)
        .map(|s| plain_text(&s.title))
        .collect::<Vec<_>>()
        .join(" ");
    let label = layout::continuation_label(doc.meta.lang.as_deref(), &sample);
    let ctx = layout::Ctx::new(&info, typo.clone(), judge);
    let mut placements = Vec::new();
    for s in &slides {
        placements.extend(layout::place(s, total, &ctx, &images, label, &mut warnings));
    }
    let title = doc
        .meta
        .title
        .clone()
        .or_else(|| slides.first().map(|s| plain_text(&s.title)))
        .unwrap_or_default();
    let input = writer::DeckInput {
        template: &package,
        info: &info,
        typo: &typo,
        slides: &placements,
        images: &images,
        footnotes: &numbers,
        lang,
        title,
        author: doc.meta.authors.join(", "),
    };
    let bytes = writer::write(&input, &mut warnings)?;
    Ok(Rendered { bytes, warnings })
}

/// The size occupancy is ranked at: the body size of the template's canonical
/// content layout (the most used "title and one text box"), raised to `--min-pt`.
fn rank_size(info: &template::TemplateInfo, typo: &fit::Typo) -> f32 {
    let ctx = layout::Ctx::new(info, typo.clone(), None);
    let size = (0..info.layouts.len())
        .find(|&i| ctx.archetypes[i] == fit::Archetype::Content && ctx.canonical(i))
        .and_then(|i| {
            let l = &info.layouts[i];
            fit::bodies(l, info)
                .first()
                .map(|&b| l.placeholders[b].size_pt(0))
        })
        .unwrap_or(18.0);
    typo.eff(size)
}

/// Mermaid blocks become pictures (an SVG data URI) with the `mermaid` feature; without
/// it, or when a diagram does not parse, they stay as their source, with a warning.
fn render_diagrams(blocks: &[Block], warnings: &mut Vec<Warning>) -> Vec<Block> {
    blocks
        .iter()
        .map(|b| match b {
            Block::Mermaid { code, .. } => diagram(code, warnings).unwrap_or_else(|| b.clone()),
            Block::Div { attr, content } => Block::Div {
                attr: attr.clone(),
                content: render_diagrams(content, warnings),
            },
            other => other.clone(),
        })
        .collect()
}

#[cfg(feature = "mermaid")]
fn diagram(code: &str, warnings: &mut Vec<Warning>) -> Option<Block> {
    use base64::Engine as _;
    match crate::mermaid::render_mermaid(code) {
        Ok(svg) => {
            let data = base64::engine::general_purpose::STANDARD.encode(svg.svg.as_bytes());
            Some(Block::Paragraph(vec![Inline::Image(crate::ast::Image {
                src: format!("data:image/svg+xml;base64,{data}"),
                alt: vec![Inline::Text("Mermaid diagram".into())],
                title: None,
                attr: crate::ast::Attr::default(),
            })]))
        }
        Err(e) => {
            warnings.push(Warning::new(
                "mermaid-not-rendered",
                format!("a Mermaid diagram is shown as its source: {e}"),
            ));
            None
        }
    }
}

#[cfg(not(feature = "mermaid"))]
fn diagram(_code: &str, warnings: &mut Vec<Warning>) -> Option<Block> {
    warnings.push(Warning::new(
        "mermaid-not-rendered",
        "a Mermaid diagram is shown as its source (build with the `mermaid` feature to draw it)",
    ));
    None
}

/// Fold the options that override the front matter into the document.
fn apply_options(doc: &Document, options: &Options) -> Document {
    let mut doc = doc.clone();
    if let Some(a) = &options.author {
        doc.meta.authors = vec![a.clone()];
    }
    if let Some(d) = &options.date {
        doc.meta.date = Some(d.clone());
    }
    if let Some(l) = &options.lang {
        doc.meta.lang = Some(l.clone());
    }
    doc
}

/// Resolve citations against the bibliography, when one is given and the bibliography
/// module can; otherwise citations print as written, with a warning.
fn apply_bibliography(doc: &Document, options: &Options, warnings: &mut Vec<Warning>) -> Document {
    let mut paths: Vec<std::path::PathBuf> = options.bibliography.clone();
    let base = options.base_dir.clone().unwrap_or_default();
    paths.extend(doc.meta.bibliography.iter().map(|p| base.join(p)));
    if paths.is_empty() {
        return doc.clone();
    }
    let mut bib = crate::bib::Bibliography::default();
    for p in &paths {
        let parsed = std::fs::read_to_string(p)
            .map_err(Error::from)
            .and_then(|src| crate::bib::parse(&src));
        match parsed {
            Ok(b) => bib.entries.extend(b.entries),
            Err(e) => {
                warnings.push(Warning::new(
                    "bibliography-not-applied",
                    format!(
                        "bibliography `{}`: {e}; citations print as written",
                        p.display()
                    ),
                ));
                return doc.clone();
            }
        }
    }
    let style = options
        .citation_style
        .as_deref()
        .or(doc.meta.citation_style.as_deref())
        .or(doc.meta.csl.as_deref())
        .and_then(crate::bib::Style::from_name)
        .unwrap_or_default();
    let french = doc
        .meta
        .lang
        .as_deref()
        .is_some_and(|l| l.to_lowercase().starts_with("fr"));
    let title = options
        .bibliography_name
        .clone()
        .or_else(|| doc.meta.reference_section_title.clone())
        .unwrap_or_else(|| if french { "Références" } else { "References" }.into());
    match crate::bib::apply(doc, &bib, style, &title) {
        Ok((d, w)) => {
            warnings.extend(w);
            d
        }
        Err(e) => {
            warnings.push(Warning::new(
                "bibliography-not-applied",
                format!("citations print as written: {e}"),
            ));
            doc.clone()
        }
    }
}

/// Footnote numbers by label (order of first reference) and the closing slide that
/// lists them, as Pandoc does.
fn footnotes(doc: &Document) -> (BTreeMap<String, usize>, Option<layout::Slide>) {
    let mut defs: BTreeMap<String, Vec<Block>> = BTreeMap::new();
    fn collect_defs(blocks: &[Block], defs: &mut BTreeMap<String, Vec<Block>>) {
        for b in blocks {
            match b {
                Block::FootnoteDef { label, content } => {
                    defs.insert(label.clone(), content.clone());
                }
                Block::Div { content, .. } | Block::BlockQuote(content) => {
                    collect_defs(content, defs)
                }
                _ => {}
            }
        }
    }
    collect_defs(&doc.blocks, &mut defs);
    let mut order: Vec<(String, Option<Vec<Inline>>)> = Vec::new();
    fn walk_inl(inl: &[Inline], order: &mut Vec<(String, Option<Vec<Inline>>)>) {
        for i in inl {
            match i {
                Inline::FootnoteRef(l) if !order.iter().any(|(x, _)| x == l) => {
                    order.push((l.clone(), None));
                }
                Inline::InlineNote(content) => {
                    order.push((format!("\u{0}inline{}", order.len()), Some(content.clone())));
                }
                Inline::Emph(x)
                | Inline::Strong(x)
                | Inline::Strikeout(x)
                | Inline::Superscript(x)
                | Inline::Subscript(x) => walk_inl(x, order),
                Inline::Link { text, .. } => walk_inl(text, order),
                _ => {}
            }
        }
    }
    fn walk(blocks: &[Block], order: &mut Vec<(String, Option<Vec<Inline>>)>) {
        for b in blocks {
            match b {
                Block::Paragraph(i) | Block::Heading { content: i, .. } => walk_inl(i, order),
                Block::BlockQuote(x) | Block::Div { content: x, .. } => walk(x, order),
                Block::List { items, .. } => {
                    for it in items {
                        walk(&it.content, order);
                    }
                }
                Block::Table { headers, rows, .. } => {
                    for c in headers.iter().chain(rows.iter().flatten()) {
                        walk_inl(c, order);
                    }
                }
                _ => {}
            }
        }
    }
    walk(&doc.blocks, &mut order);
    if order.is_empty() {
        return (BTreeMap::new(), None);
    }
    let mut numbers = BTreeMap::new();
    let mut items = Vec::new();
    for (n, (label, inline)) in order.iter().enumerate() {
        numbers.insert(label.clone(), n + 1);
        let content = match inline {
            Some(c) => vec![Block::Paragraph(c.clone())],
            None => defs.get(label).cloned().unwrap_or_default(),
        };
        items.push(crate::ast::ListItem {
            task: None,
            content: if content.is_empty() {
                vec![Block::Paragraph(vec![Inline::Text(label.clone())])]
            } else {
                content
            },
        });
    }
    let slide = layout::Slide {
        index: 0,
        title: vec![Inline::Text("Notes".into())],
        level: 2,
        attr: crate::ast::Attr::default(),
        body: vec![Block::List {
            ordered: true,
            start: 1,
            items,
        }],
        notes: Vec::new(),
        title_only: false,
        subtitle: Vec::new(),
    };
    (numbers, Some(slide))
}

/// Build a `.pptx` byte buffer from a document's blocks, with no front matter.
///
/// # Errors
///
/// As [`render`].
pub fn build(blocks: &[Block]) -> Result<Vec<u8>> {
    let doc = Document {
        meta: Metadata::default(),
        blocks: blocks.to_vec(),
        warnings: Vec::new(),
    };
    render(&doc, &Options::default()).map(|r| r.bytes)
}
