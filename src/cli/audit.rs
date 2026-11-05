//! `md2star-rs audit` (owned by the PPTX port).

use std::fs;
use std::path::PathBuf;

use clap::Args;

use crate::audit::{audit, AuditOptions};
use crate::error::{Error, Result};

/// Flags of `md2star-rs audit`.
#[derive(Args, Debug, Clone)]
pub struct AuditArgs {
    /// The deck to audit.
    #[arg(value_name = "DECK.pptx")]
    pub deck: PathBuf,
    /// Machine-readable findings.
    #[arg(long)]
    pub json: bool,
    /// Also fail on `overrun`, which is otherwise an observation.
    #[arg(long)]
    pub strict: bool,
    /// Also fail on slide content set under N points (Kawasaki's 10/20/30: pass 30).
    /// Footer chrome is exempt.
    #[arg(long, value_name = "N")]
    pub min_pt: Option<f32>,
}

/// Run `audit`.
///
/// # Errors
///
/// The deck cannot be read or audited; [`Error::Unsupported`] until the audit is ported.
pub fn run(args: &AuditArgs) -> Result<()> {
    let bytes = fs::read(&args.deck)?;
    let options = AuditOptions {
        min_pt: args.min_pt,
        strict: args.strict,
    };
    let report = audit(&bytes, &options)?;
    // The report's rendering (text and --json) and the exit status on findings are the
    // PPTX port's to write; until then a report can only come back empty.
    if !report.findings.is_empty() {
        return Err(Error::unsupported("printing audit findings"));
    }
    let _ = args.json;
    println!(
        "{}: {} slide(s), no findings",
        args.deck.display(),
        report.slides
    );
    Ok(())
}
