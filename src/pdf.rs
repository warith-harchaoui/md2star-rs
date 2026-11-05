//! PDF output, in process — no LibreOffice, no LaTeX, no subprocess (feature `pdf`).
//!
//! The document is written as [Typst](https://typst.app) markup ([`to_typst`]) and
//! compiled in memory by the `typst` crates, with the fonts embedded in the binary
//! (`typst-assets`: Libertinus Serif for text, New Computer Modern Math for formulas,
//! DejaVu Sans Mono for code). No temporary file, no system font, no clock: the same
//! document gives the same PDF bytes on every run and every machine.
//!
//! What Typst brings that a hand-made layout would have to rebuild: Knuth–Plass line
//! breaking with hyphenation for the document's `lang` (French and English included),
//! native math typesetting (LaTeX formulas are translated to Typst math, see
//! `pdf/math.rs`), footnotes at the foot of their page, tables that break across pages,
//! figures with captions, a table of contents (`--toc`), tagged (accessible) PDF.
//!
//! # Why Typst, and what it costs
//!
//! Measured on 2026-11-05 against the alternative of laying pages out ourselves on
//! `krilla` (the PDF writer Typst itself uses) or `printpdf`:
//!
//! - **Pure Rust**: yes, but for one crate: `psm` (pulled by `stacker`, which Typst's
//!   evaluator uses against deep recursion) assembles a few lines of stack-switching
//!   code with the target's C toolchain. iOS builds have it (Xcode); an Android build
//!   needs the NDK's clang, which every Android build already has. A host-only
//!   `cargo check --target aarch64-linux-android` works with `CC_aarch64_linux_android=clang`.
//! - **Compiles for `aarch64-apple-ios` and `aarch64-linux-android`**: yes (`cargo check`).
//! - **MSRV**: Typst 0.15 needs Rust 1.92. The crate's own MSRV stays 1.88: the `pdf`
//!   feature alone raises it, which the README and `Cargo.toml` say.
//! - **Binary size**: the stripped release binary (macOS arm64) grows from 6.0 MB to
//!   50.4 MB; about 9.6 MB of that are the embedded fonts, most of the rest Typst's own
//!   data (hyphenation patterns, Unicode tables, syntax definitions for code).
//! - **The alternative**: `krilla` or `printpdf` give a PDF writer, not a typesetter:
//!   line breaking, hyphenation, math, footnotes and table layout would all be ours to
//!   write, for a worse result. Typst does them well and is maintained.
//!
//! Owned by the twin + PDF port (see [`crate::twin`]).

mod markup;
mod math;
mod world;

use crate::ast::{Document, Warning};
use crate::error::{Error, Result};
use crate::options::{Options, Rendered};

/// Render `doc` to PDF bytes.
///
/// Citations are resolved first (`--bib`, as for DOCX). A formula outside the
/// supported LaTeX subset is shown as text with a warning; if Typst still rejects one,
/// the document is set again with every formula as its LaTeX source, with a warning,
/// rather than failing.
///
/// # Errors
///
/// [`Error::Bib`] when a bibliography cannot be read; [`Error::Pdf`] when Typst cannot
/// set the document (the message names the generated line).
pub fn render(doc: &Document, options: &Options) -> Result<Rendered> {
    let (doc, bib_warnings) = crate::bib::resolve(doc, options)?;
    let mut warnings = doc.warnings.clone();
    warnings.extend(bib_warnings);
    if options.reference_doc.is_some() {
        warnings.push(Warning::new(
            "not-yet-ported",
            "PDF: --reference-doc is not applied; the PDF is set with its own typography",
        ));
    }
    let first = markup::build(&doc, options, markup::MathMode::Native);
    let compiled = world::compile(&first.source, &first.files);
    let (markup, compiled) = match compiled.pdf {
        Err(_) if first.has_math => {
            let second = markup::build(&doc, options, markup::MathMode::Source);
            let again = world::compile(&second.source, &second.files);
            if again.pdf.is_ok() {
                warnings.push(Warning::new(
                    "math-fallback",
                    format!(
                        "a formula could not be typeset ({}); every formula is shown as its LaTeX source",
                        compiled.pdf.err().unwrap_or_default().join("; ")
                    ),
                ));
            }
            (second, again)
        }
        _ => (first, compiled),
    };
    warnings.extend(markup.warnings);
    warnings.extend(
        compiled
            .warnings
            .into_iter()
            .map(|w| Warning::new("typst", w)),
    );
    match compiled.pdf {
        Ok(bytes) => Ok(Rendered { bytes, warnings }),
        Err(errors) => Err(Error::Pdf(errors.join("; "))),
    }
}

/// The Typst markup [`render`] compiles, and the warnings raised while writing it —
/// for inspecting or post-editing the typesetting. Images are referenced under
/// `/media/…` and are not returned.
///
/// # Errors
///
/// [`Error::Bib`] when a bibliography cannot be read.
pub fn to_typst(doc: &Document, options: &Options) -> Result<(String, Vec<Warning>)> {
    let (doc, mut warnings) = crate::bib::resolve(doc, options)?;
    let markup = markup::build(&doc, options, markup::MathMode::Native);
    warnings.extend(markup.warnings);
    Ok((markup.source, warnings))
}
