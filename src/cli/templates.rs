//! `md2star-rs templates` (owned by the PPTX port).

use clap::{Args, Subcommand};

use crate::error::Result;

/// Flags of `md2star-rs templates`.
#[derive(Args, Debug, Clone)]
pub struct TemplatesArgs {
    /// What to do.
    #[command(subcommand)]
    pub action: TemplatesAction,
}

/// `templates` subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum TemplatesAction {
    /// List the templates compiled into this binary.
    List,
    /// Write a bundled template to a file, to edit it or pass it to --reference-doc.
    Export {
        /// `docx` or `pptx`.
        format: String,
        /// Where to write it.
        output: std::path::PathBuf,
    },
}

/// Run `templates`.
///
/// # Errors
///
/// [`crate::Error::Unsupported`] until templates are bundled.
pub fn run(args: &TemplatesArgs) -> Result<()> {
    let templates = crate::templates::bundled()?;
    match &args.action {
        TemplatesAction::List => {
            for t in templates {
                println!(
                    "{}\t{}\t{} bytes",
                    t.format.extension(),
                    t.name,
                    t.bytes.len()
                );
            }
            Ok(())
        }
        TemplatesAction::Export { format, output } => {
            let found = templates
                .into_iter()
                .find(|t| t.format.extension() == format.as_str())
                .ok_or_else(|| {
                    crate::Error::unsupported(format!("no bundled `{format}` template"))
                })?;
            std::fs::write(output, found.bytes)?;
            println!("wrote {}", output.display());
            Ok(())
        }
    }
}
