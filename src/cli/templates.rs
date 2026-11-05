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
    /// List the layouts of a .pptx template (default: the bundled one): name, role,
    /// placeholders with their boxes and text sizes — what the layout selector sees.
    Layouts {
        /// A .pptx template; the bundled default when omitted.
        template: Option<std::path::PathBuf>,
    },
}

/// Run `templates`.
///
/// # Errors
///
/// The template cannot be read, or the export cannot be written.
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
        TemplatesAction::Layouts { template } => {
            let bytes = match template {
                Some(path) => std::fs::read(path)?,
                None => crate::templates::DEFAULT_PPTX.to_vec(),
            };
            let info = crate::pptx::template::inspect(&bytes)?;
            let (w, h) = info.slide_size;
            println!(
                "slide {:.2} x {:.2} in; theme fonts: {} / {}",
                w as f64 / 914_400.0,
                h as f64 / 914_400.0,
                info.theme_fonts.0.as_deref().unwrap_or("-"),
                info.theme_fonts.1.as_deref().unwrap_or("-")
            );
            for layout in &info.layouts {
                let role = crate::pptx::fit::archetype(layout, info.slide_size);
                println!("{}\t{}", layout.name, role.label());
                for p in layout.placeholders.iter().filter(|p| !p.is_latent()) {
                    let frame = p.frame.map_or_else(
                        || "inherited box".to_string(),
                        |(x, y, cx, cy)| {
                            format!(
                                "{:.0}%,{:.0}% {:.0}%x{:.0}%",
                                100.0 * x as f64 / w as f64,
                                100.0 * y as f64 / h as f64,
                                100.0 * cx as f64 / w as f64,
                                100.0 * cy as f64 / h as f64
                            )
                        },
                    );
                    println!(
                        "\t{} idx={}\t{frame}\t{} pt{}",
                        p.kind,
                        p.idx,
                        p.size_pt(0),
                        if p.vertical { " (vertical)" } else { "" }
                    );
                }
            }
            Ok(())
        }
    }
}
