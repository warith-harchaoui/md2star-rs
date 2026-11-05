//! PPTX writer: [`Document`] → `.pptx` bytes.
//!
//! # What this module must do (phase 2: the PPTX port)
//!
//! Reach parity with `md2star pptx`, in process and in pure Rust: real slides built on a
//! template's masters and layouts ([`template`], `--reference-doc` / a designer `.pptx`),
//! deterministic layout selection ([`layout`]: explicit `{layout="Name"}`, the classes
//! `.statement` `.bignumber` `.quote` `.recap` `.agenda` `.section` `.closing`, a `#`
//! after the cover is a section, a lone figure, a quote, a short sentence), `--min-pt N`
//! (no text under N points: change layout or split the slide, never shrink), speaker
//! notes ([`crate::ast::Block::Notes`]), two columns (`::: columns`), tables, code,
//! images ([`images`], fed by [`crate::resources`]) and line splitting. Layout selection
//! by a language model sits behind the `llm` feature (`src/llm.rs`); without it the
//! deterministic choice must stay complete and good.
//!
//! # Slide segmentation (Pandoc's rule, as md2star applies it)
//!
//! Every thematic break ([`crate::ast::Block::ThematicBreak`]) ends a slide and every
//! `#`/`##` heading starts one. A front-matter `title` no slide carries becomes a cover.
//!
//! # Files this module owns
//!
//! `src/pptx/mod.rs` (this writer), `src/pptx/layout.rs`, `src/pptx/template.rs`,
//! `src/pptx/images.rs`, `src/audit.rs`, `src/llm.rs`, `src/templates.rs`,
//! `src/cli/pptx.rs`, `src/cli/audit.rs`, `src/cli/templates.rs`, and the
//! `tests/pptx_*.rs` / `tests/audit_*.rs` integration tests. Math reuses
//! [`crate::docx::math::latex_to_omml`] and citations reuse [`crate::bib`] (read-only).
//!
//! # State at the end of phase 1
//!
//! The v0.4 writer, adapted to the full AST and moved to `legacy.rs` behind the default
//! feature `legacy-pptx`: each `#` heading starts a slide and the body is flattened to
//! bullets through `ppt-rs`; speaker notes are kept off the slide. [`render`] warns about
//! every option it does not honour yet. `ppt-rs` brings C code (bzip2, zstd, through
//! `zip` 0.6), so replacing it with a pure-Rust writer is part of this port; without the
//! feature, [`build`] is [`crate::Error::Unsupported`].
//!
//! # Determinism
//!
//! `ppt-rs` stamps the wall-clock time into `docProps/core.xml`; `normalize` pins it to
//! a fixed sentinel and re-packs the zip with a fixed entry mtime, so identical Markdown
//! yields byte-identical `.pptx` output on every run (see `tests/pptx.rs`).

pub mod images;
pub mod layout;
#[cfg(feature = "legacy-pptx")]
mod legacy;
pub mod template;

use crate::ast::{Block, Document, Warning};
use crate::error::Result;
use crate::options::{Options, Rendered};

/// Convert a parsed document to `.pptx` bytes.
///
/// The entry point every caller goes through. Options this writer does not honour yet
/// produce a `not-yet-ported` warning rather than being ignored silently.
///
/// # Errors
///
/// [`crate::Error::Pptx`] when the deck cannot be assembled or packed.
pub fn render(doc: &Document, options: &Options) -> Result<Rendered> {
    let mut warnings = doc.warnings.clone();
    let mut unported = |what: &str| {
        warnings.push(Warning::new(
            "not-yet-ported",
            format!("PPTX: {what} is not ported to md2star-rs yet and was ignored"),
        ));
    };
    if options.reference_doc.is_some() {
        unported("the reference template (--reference-doc)");
    }
    if options.min_pt.is_some() {
        unported("the minimum text size (--min-pt)");
    }
    if options.smart_layout {
        unported("model-driven layout selection (--smart-layout)");
    }
    if !options.bibliography.is_empty() || !doc.meta.bibliography.is_empty() {
        unported("the bibliography (--bib)");
    }
    Ok(Rendered {
        bytes: build(&doc.blocks)?,
        warnings,
    })
}

/// Build a `.pptx` byte buffer from a document's blocks (the v0.4 bullet deck).
///
/// # Errors
///
/// [`crate::Error::Pptx`] if the deck cannot be assembled or packed.
#[cfg(feature = "legacy-pptx")]
pub fn build(blocks: &[Block]) -> Result<Vec<u8>> {
    legacy::build(blocks)
}

/// Without `legacy-pptx` there is no PPTX writer until the pure-Rust one lands.
///
/// # Errors
///
/// Always [`crate::Error::Unsupported`].
#[cfg(not(feature = "legacy-pptx"))]
pub fn build(blocks: &[Block]) -> Result<Vec<u8>> {
    let _ = blocks;
    Err(crate::Error::unsupported(
        "PPTX output in a build without the `legacy-pptx` feature: the pure-Rust \
         PresentationML writer is not ported yet",
    ))
}
