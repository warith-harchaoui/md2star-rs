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

/// Run `audit`: print every finding (or the JSON array) and the summary line, then
/// exit as `md2star audit` does — 0 when nothing but observations were found, 1 on a
/// defect (an overrun counts only with `--strict`), 2 when the deck cannot be read.
///
/// # Errors
///
/// None in practice: every failure ends the process with its own status.
pub fn run(args: &AuditArgs) -> Result<()> {
    use std::io::Write as _;
    if !args.deck.is_file() {
        eprintln!("md2star-rs audit: no such file: {}", args.deck.display());
        std::process::exit(2);
    }
    let options = AuditOptions {
        min_pt: args.min_pt,
        strict: args.strict,
    };
    let report = match fs::read(&args.deck)
        .map_err(Error::from)
        .and_then(|bytes| audit(&bytes, &options))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("md2star-rs audit: cannot read {}: {e}", args.deck.display());
            std::process::exit(2);
        }
    };
    if args.json {
        out!("{}", report.to_json());
    } else {
        for finding in &report.findings {
            out!("{finding}");
        }
        out!("{}", report.summary());
    }
    let _ = std::io::stdout().flush();
    if report.fails(args.strict) {
        std::process::exit(1);
    }
    Ok(())
}
