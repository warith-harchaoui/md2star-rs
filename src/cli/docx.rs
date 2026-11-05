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
    /// A CSL style file, as Pandoc's `--csl`. The file is not read: its name picks a
    /// built-in style (ieee.csl, vancouver.csl, nature.csl → numeric; apa.csl,
    /// chicago-author-date.csl, harvard.csl → author-date). `--citation-style` wins.
    #[arg(long, value_name = "FILE")]
    pub csl: Option<String>,
}

/// Run `docx`.
///
/// # Errors
///
/// Reading, converting or writing failed.
pub fn run(args: &DocxArgs) -> Result<()> {
    let mut options = args.common.options()?;
    if options.citation_style.is_none() {
        options.citation_style = args.csl.clone();
    }
    convert_file(&args.common, Format::Docx, &options)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: DocxArgs,
    }

    #[test]
    fn csl_selects_the_style_unless_citation_style_is_given() {
        let cli = Cli::try_parse_from(["md2docx", "in.md", "--csl", "styles/ieee.csl"]).unwrap();
        assert_eq!(cli.args.csl.as_deref(), Some("styles/ieee.csl"));
        let cli = Cli::try_parse_from([
            "md2docx",
            "in.md",
            "--csl",
            "ieee.csl",
            "--citation-style",
            "author-date",
        ])
        .unwrap();
        assert_eq!(
            cli.args.common.citation_style.as_deref(),
            Some("author-date")
        );
    }
}
