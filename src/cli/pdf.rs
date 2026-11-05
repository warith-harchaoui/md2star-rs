//! `md2star-rs pdf` (owned by the twin + PDF port).

use clap::Args;

use super::{convert_file, ConvertArgs};
use crate::error::Result;
use crate::options::Format;

/// Flags of `md2star-rs pdf`.
#[derive(Args, Debug, Clone)]
pub struct PdfArgs {
    /// The flags every conversion shares.
    #[command(flatten)]
    pub common: ConvertArgs,
}

/// Run `pdf`. Without the `pdf` feature the library answers "not ported yet / not
/// compiled in", which is printed as is.
///
/// # Errors
///
/// Reading, converting or writing failed.
pub fn run(args: &PdfArgs) -> Result<()> {
    let options = args.common.options()?;
    convert_file(&args.common, Format::Pdf, &options)
}
