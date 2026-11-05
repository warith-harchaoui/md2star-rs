//! `audit`: what is wrong with a deck, read off its OOXML.
//!
//! # What this module must do
//!
//! Port `md2star audit` (`md2star/pptx_audit.py`), deterministic and portable: no
//! model, no render, no network — pure XML over the zip, so it runs on a phone. It works
//! on any `.pptx`, not only the ones md2star built. For every slide, with each shape's
//! box resolved through its layout and master placeholders:
//!
//! - **overlap**: two text-bearing shapes whose boxes intersect;
//! - **off-canvas**: a shape (partly) outside the slide;
//! - **unreadable**: text set under a legibility floor;
//! - **below-min-pt**: text under `--min-pt N` (footer chrome — date, footer, slide
//!   number, fields, one line at the bottom — exempt);
//! - **image-over-text**: a picture covering a text shape;
//! - **overrun**: text that cannot fit its box at its size (an observation; a failure
//!   only with `--strict`).
//!
//! Output: human-readable lines, or JSON with `--json`.
//!
//! Owned by the PPTX port (see [`crate::pptx`]).

use crate::error::{Error, Result};

/// What `audit` checks against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditOptions {
    /// `--min-pt N`: also fail on slide content set under N points.
    pub min_pt: Option<f32>,
    /// `--strict`: also fail on `overrun`.
    pub strict: bool,
}

/// The kind of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// Two text shapes overlap.
    Overlap,
    /// A shape lies (partly) outside the slide.
    OffCanvas,
    /// Text below the legibility floor.
    Unreadable,
    /// Text below `--min-pt`.
    BelowMinPt,
    /// A picture covers text.
    ImageOverText,
    /// Text overruns its box (an observation unless `--strict`).
    Overrun,
}

/// One problem on one slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// 1-based slide number.
    pub slide: usize,
    /// What is wrong.
    pub kind: FindingKind,
    /// The shape(s) involved, by name (`Title 1`, `Picture 3`).
    pub shapes: Vec<String>,
    /// A one-line human explanation.
    pub message: String,
}

/// The result of auditing a deck.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditReport {
    /// How many slides were read.
    pub slides: usize,
    /// Every finding, by slide then position.
    pub findings: Vec<Finding>,
}

/// Audit a `.pptx` given as bytes.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — the audit is not ported yet. Once ported:
/// [`Error::Read`] when the bytes are not a readable `.pptx`.
pub fn audit(pptx: &[u8], options: &AuditOptions) -> Result<AuditReport> {
    let _ = (pptx, options);
    Err(Error::unsupported("audit"))
}
