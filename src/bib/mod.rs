//! Bibliographies: BibTeX in, formatted citations and reference lists out.
//!
//! The format-independent half of `--bib`, shared by the DOCX, PPTX and PDF writers:
//!
//! - **Parse** BibTeX ([`parse`]): `@type{key, field = {…} | "…" | 123}`, nested braces,
//!   `#` concatenation, `@string` macros, `@comment`/`@preamble` skipped, LaTeX accents
//!   (`{\'e}`, `\"o`) and `--` decoded ([`decode_latex`]), names split (`Last, First and
//!   Other, A.`, `First von Last`, `von Last, Jr, First`), `{Protected Case}` kept.
//! - **Render**: two built-in styles. [`Style::AuthorDate`] follows Chicago author-date,
//!   Pandoc's default (`(Pearl 2009, 3; see also Pearl 2000)`, `Pearl (2009, chap. 2)`,
//!   `(2000)`, `et al.` from four authors, `2009a`/`2009b` disambiguation, the reference
//!   list sorted by author then year with `———.` for a repeated author list).
//!   [`Style::Numeric`] follows IEEE (`[1]`, `[1], [2]`, `[1, p. 3]`, numbered in order
//!   of first citation, `[n] J. Pearl, *Title*, 2nd ed. Publisher, 2009.`). Words
//!   (`and`/`et`, `ed.`/`éd.`, `n.d.`/`s. d.`…) follow the document's `lang`.
//! - **Apply** ([`apply`], or [`resolve`] which also reads the files and picks the style
//!   and title from the options and the front matter): replace every
//!   [`crate::ast::Inline::Cite`] by its rendered text and put the reference list under
//!   the bibliography title — in the `::: {#refs}` div when the document has one, at the
//!   end otherwise. An unknown key never fails the conversion: it renders as written, in
//!   bold (`**[@key]**`), with a warning.
//!
//! The reference list comes back as ordinary AST: a level-1 heading and a
//! `Div {#refs .references}` holding one `Div {#ref-KEY .csl-entry custom-style=Bibliography}`
//! per entry, so every writer can render it and the DOCX writer gives it Word's
//! `Bibliography` paragraph style and a bookmark per entry.
//!
//! # What is not done
//!
//! Full CSL is out of scope: a `csl:` file is never read. Its *name* selects a built-in
//! style (`ieee.csl` → numeric, `apa.csl` → author-date); any other name falls back to
//! author-date with a warning. Compared with Pandoc's citeproc, titles are not
//! re-cased (citeproc title-cases English titles in Chicago and sentence-cases them in
//! IEEE), page ranges are not abbreviated (`770–778`, not `770–78`), and authors sharing
//! a family name are not told apart by their initials.
//!
//! Owned by the DOCX port (see [`crate::docx`]); the PPTX and PDF writers call
//! [`resolve`] (or [`apply`]) read-only.

mod parse;
mod render;

pub use parse::{decode_latex, parse};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ast::{Document, Warning};
use crate::error::{Error, Result};
use crate::options::Options;

/// One person's name, split the BibTeX way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    /// First names (`Judea`, `Charles F. Van`).
    pub given: String,
    /// The lower-case "von" part (`de`, `van der`), empty when absent.
    pub particle: String,
    /// The family name (`Pearl`). A corporate author (`{Barnes and Noble}`) is all
    /// family name.
    pub family: String,
    /// `Jr.`, `III`…, empty when absent.
    pub suffix: String,
    /// `true` for BibTeX's `and others` (rendered as "et al.").
    pub others: bool,
}

/// One BibTeX entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The citation key.
    pub key: String,
    /// The entry type, lower-cased (`book`, `article`, `inproceedings`…).
    pub kind: String,
    /// The fields, lower-cased names, values with braces and LaTeX escapes decoded.
    /// `author` and `editor` are also kept here as decoded text.
    pub fields: BTreeMap<String, String>,
    /// The `author` field, split into names.
    pub authors: Vec<Name>,
    /// The `editor` field, split into names.
    pub editors: Vec<Name>,
}

impl Entry {
    /// A field's decoded value, `None` when absent or empty.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

/// A parsed bibliography: entries in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bibliography {
    /// The entries.
    pub entries: Vec<Entry>,
}

impl Bibliography {
    /// The entry for `key`, if any (the first one when a key is defined twice, as
    /// BibTeX does).
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Append another bibliography's entries (several `--bib` files).
    pub fn extend(&mut self, other: Bibliography) {
        self.entries.extend(other.entries);
    }
}

/// The built-in citation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// `(Pearl 2009)` — Chicago author-date, Pandoc's and md2star's default.
    #[default]
    AuthorDate,
    /// `[1]` — IEEE, numbered in order of first citation.
    Numeric,
}

impl Style {
    /// The style a name designates (`author-date`, `numeric`, or a CSL file name such
    /// as `ieee.csl` / `apa.csl`); `None` when the name says nothing recognisable.
    ///
    /// # Examples
    ///
    /// ```
    /// use md2star_rs::bib::Style;
    /// assert_eq!(Style::from_name("ieee.csl"), Some(Style::Numeric));
    /// assert_eq!(Style::from_name("Author-Date"), Some(Style::AuthorDate));
    /// assert_eq!(Style::from_name("mystery"), None);
    /// ```
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.trim().to_ascii_lowercase();
        let stem = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
        let stem = stem.strip_suffix(".csl").unwrap_or(stem);
        match stem {
            "numeric"
            | "numbered"
            | "ieee"
            | "vancouver"
            | "nature"
            | "acm"
            | "acm-sig-proceedings"
            | "springer-basic-brackets" => Some(Style::Numeric),
            "author-date"
            | "authordate"
            | "author-year"
            | "apa"
            | "chicago"
            | "chicago-author-date"
            | "harvard"
            | "harvard-cite-them-right" => Some(Style::AuthorDate),
            _ => None,
        }
    }
}

/// Resolve every citation of `doc` against `bib` in `style`, put the reference list
/// under the heading `title` (none when `title` is empty), and return the rewritten
/// document with its warnings. Words follow `doc.meta.lang`.
///
/// # Errors
///
/// None today; the `Result` keeps room for a style that can fail.
pub fn apply(
    doc: &Document,
    bib: &Bibliography,
    style: Style,
    title: &str,
) -> Result<(Document, Vec<Warning>)> {
    Ok(render::apply(doc, bib, style, title))
}

/// Read and parse every BibTeX file of `paths`, in order, into one bibliography.
///
/// # Errors
///
/// [`Error::Bib`] naming the file when one cannot be read or parsed.
pub fn load(paths: &[PathBuf]) -> Result<Bibliography> {
    let mut bib = Bibliography::default();
    for path in paths {
        let source = std::fs::read_to_string(path)
            .map_err(|e| Error::Bib(format!("{}: {e}", path.display())))?;
        let parsed = parse(&source).map_err(|e| match e {
            Error::Bib(msg) => Error::Bib(format!("{}: {msg}", path.display())),
            other => other,
        })?;
        bib.extend(parsed);
    }
    Ok(bib)
}

/// The bibliography files a conversion uses: `options.bibliography` (as given), then the
/// front matter's `bibliography` (relative to `options.base_dir`).
pub fn files(doc: &Document, options: &Options) -> Vec<PathBuf> {
    let mut paths = options.bibliography.clone();
    for name in &doc.meta.bibliography {
        let path = Path::new(name);
        let path = match &options.base_dir {
            Some(base) if path.is_relative() => base.join(path),
            _ => path.to_path_buf(),
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// The heading md2star puts before the bibliography when nothing names it, per language
/// (the Python md2star's table).
pub fn default_title(lang: Option<&str>) -> &'static str {
    match lang
        .and_then(|l| l.split(['-', '_']).next())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("fr") => "Bibliographie",
        Some("es") => "Bibliografía",
        Some("de") => "Literatur",
        Some("it" | "pt") => "Bibliografia",
        Some("nl") => "Bibliografie",
        Some("ru") => "Библиография",
        _ => "Bibliography",
    }
}

/// Everything `--bib` does, from options to rewritten document: read the files
/// ([`files`], [`load`]), pick the style (`options.citation_style`, then the front
/// matter's `citation-style`, then its `csl` file *name*), pick the heading
/// (`options.bibliography_name`, then `reference-section-title`, then
/// [`default_title`]), and [`apply`].
///
/// Without any bibliography file the document comes back unchanged (citations stay as
/// written, as with Pandoc without `--citeproc`).
///
/// # Errors
///
/// [`Error::Bib`] when a bibliography file cannot be read or parsed.
pub fn resolve(doc: &Document, options: &Options) -> Result<(Document, Vec<Warning>)> {
    let paths = files(doc, options);
    if paths.is_empty() {
        return Ok((doc.clone(), Vec::new()));
    }
    let bib = load(&paths)?;
    let mut warnings = Vec::new();
    let requested = options
        .citation_style
        .clone()
        .or_else(|| doc.meta.citation_style.clone())
        .or_else(|| doc.meta.csl.clone());
    let style = match requested.as_deref() {
        None => Style::default(),
        Some(name) => Style::from_name(name).unwrap_or_else(|| {
            warnings.push(Warning::new(
                "citation-style-unknown",
                format!(
                    "citation style `{name}` is not built in (CSL files are not read): \
                     using author-date; built in are author-date (chicago, apa, harvard) \
                     and numeric (ieee, vancouver, nature, acm)"
                ),
            ));
            Style::default()
        }),
    };
    let mut doc = doc.clone();
    if let Some(lang) = &options.lang {
        doc.meta.lang = Some(lang.clone());
    }
    let title = options
        .bibliography_name
        .clone()
        .or_else(|| doc.meta.reference_section_title.clone())
        .unwrap_or_else(|| default_title(doc.meta.lang.as_deref()).to_string());
    let (doc, more) = apply(&doc, &bib, style, &title)?;
    warnings.extend(more);
    Ok((doc, warnings))
}
