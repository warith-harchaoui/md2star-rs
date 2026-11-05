//! Bibliographies: BibTeX in, formatted citations and reference lists out.
//!
//! # What this module must do
//!
//! The format-independent half of `--bib`, shared by the DOCX, PPTX and PDF writers:
//!
//! - **Parse** BibTeX ([`parse`], in `parse.rs`): `@type{key, field = {…} | "…" | 123}`,
//!   nested braces, `#` concatenation, `@string` macros, `@comment`/`@preamble`
//!   skipped, LaTeX accents (`{\'e}`, `\"o`) and `--` decoded, names split
//!   (`Last, First and Other, A.` and `First Last`), `{Protected Case}` kept.
//! - **Render** (`render.rs`): the author-date style (`(Pearl, 2009)`, `Pearl (2009)`,
//!   `(Pearl, 2009, p. 3)`, `et al.` from three authors, `2009a`/`2009b` disambiguation)
//!   and the numeric style (`[1]`, `[1, 3]`, numbered in order of first citation); the
//!   reference list sorted accordingly; localised words (`p.`, `et al.`, `and`/`et`) from
//!   the document's `lang`.
//! - **Apply** ([`apply`]): replace every [`crate::ast::Inline::Cite`] by its rendered
//!   form and append the reference list under the bibliography title, warning — never
//!   failing — on an unknown key (it then renders as written, `[@key]`).
//!
//! Full CSL is out of scope: a `csl:` file's *name* may select one of the built-in
//! styles; anything else is a warning that says so.
//!
//! Owned by the DOCX port (see [`crate::docx`]); the PPTX and PDF writers call
//! [`apply`] read-only.

mod parse;
mod render;

pub use parse::parse;

use std::collections::BTreeMap;

use crate::ast::{Document, Warning};
use crate::error::Result;

/// One BibTeX entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The citation key.
    pub key: String,
    /// The entry type, lower-cased (`book`, `article`, `inproceedings`…).
    pub kind: String,
    /// The fields, lower-cased names, values with braces and LaTeX escapes decoded.
    pub fields: BTreeMap<String, String>,
}

/// A parsed bibliography: entries in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bibliography {
    /// The entries.
    pub entries: Vec<Entry>,
}

impl Bibliography {
    /// The entry for `key`, if any.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

/// The built-in citation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// `(Pearl, 2009)` — Chicago author-date, md2star's default.
    #[default]
    AuthorDate,
    /// `[1]` — numbered in order of first citation.
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
        let lower = name.to_ascii_lowercase();
        let stem = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
        let stem = stem.strip_suffix(".csl").unwrap_or(stem);
        match stem {
            "numeric"
            | "ieee"
            | "vancouver"
            | "nature"
            | "acm"
            | "acm-sig-proceedings"
            | "springer-basic-brackets" => Some(Style::Numeric),
            "author-date"
            | "authordate"
            | "apa"
            | "chicago-author-date"
            | "harvard"
            | "harvard-cite-them-right" => Some(Style::AuthorDate),
            _ => None,
        }
    }
}

/// Resolve every citation of `doc` against `bib` in `style`, append the reference list
/// titled `title`, and return the rewritten document with its warnings.
///
/// # Errors
///
/// Today: always [`crate::Error::Unsupported`] — citation rendering is not ported yet.
pub fn apply(
    doc: &Document,
    bib: &Bibliography,
    style: Style,
    title: &str,
) -> Result<(Document, Vec<Warning>)> {
    render::apply(doc, bib, style, title)
}
