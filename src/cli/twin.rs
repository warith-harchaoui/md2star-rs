//! `md2star-rs twin` (owned by the twin + PDF port).

use std::fs;
use std::path::PathBuf;

use clap::Args;

use super::{report_warnings, Verbosity};
use crate::error::{Error, Result};
use crate::twin::{SourceKind, TwinOptions};

/// Flags of `md2star-rs twin`.
#[derive(Args, Debug, Clone)]
pub struct TwinArgs {
    /// The document to read back (.docx or .pptx).
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
    /// Output folder (default: the input's).
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Prose and tables only; drop embedded images.
    #[arg(long)]
    pub no_images: bool,
    /// -v / -q.
    #[command(flatten)]
    pub verbosity: Verbosity,
}

/// Run `twin`.
///
/// # Errors
///
/// The input cannot be read, its kind is unknown, or the reader fails.
pub fn run(args: &TwinArgs) -> Result<()> {
    let ext = args
        .input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let kind = match ext.as_deref() {
        Some("docx") => SourceKind::Docx,
        Some("pptx") => SourceKind::Pptx,
        Some("pdf") => SourceKind::Pdf,
        _ => {
            return Err(Error::unsupported(format!(
                "twin reads .docx and .pptx; `{}` is neither",
                args.input.display()
            )))
        }
    };
    let bytes = fs::read(&args.input)?;
    let options = TwinOptions {
        images: !args.no_images,
        ..TwinOptions::default()
    };
    let twin = crate::twin::to_markdown(&bytes, kind, &options)?;
    let dir = args
        .out
        .clone()
        .unwrap_or_else(|| args.input.parent().map(PathBuf::from).unwrap_or_default());
    fs::create_dir_all(&dir)?;
    for (path, data) in &twin.media {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, data)?;
    }
    let stem = args
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "twin".into());
    let output = dir.join(format!("{stem}.md"));
    fs::write(&output, &twin.markdown)?;
    report_warnings(&twin.warnings, args.verbosity);
    if !args.verbosity.quiet {
        println!("wrote {}", output.display());
    }
    Ok(())
}
