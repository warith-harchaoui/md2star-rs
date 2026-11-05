//! `md2star-rs docx` / `md2docx` (owned by the DOCX port).

use clap::Args;

use super::{convert_file, ConvertArgs};
use crate::error::Result;
use crate::options::Format;

/// Flags of `md2star-rs docx`.
#[derive(Args, Debug, Clone)]
pub struct DocxArgs {
    /// The flags every conversion shares.
    #[command(flatten)]
    pub common: ConvertArgs,
}

/// Run `docx`.
///
/// # Errors
///
/// Reading, converting or writing failed.
pub fn run(args: &DocxArgs) -> Result<()> {
    let options = args.common.options()?;
    convert_file(&args.common, Format::Docx, &options)
}
