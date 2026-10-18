//! `md2star-rs` — one binary named after the crate, covering both backends.
//!
//! `md2docx` and `md2pptx` came first and are kept exactly as they are, because
//! removing or renaming them would break every existing install. The trouble is
//! that the Python `md2star` installs console scripts by those same two names:
//! with both on one machine, `PATH` order decides which runs and nothing tells
//! the caller which it was.
//!
//! Documenting that is not a fix, so this adds the missing option rather than
//! taking one away. `md2star-rs docx` and `md2star-rs pptx` cannot collide with
//! anything the Python side installs, which makes them the right choice in a
//! script, a Dockerfile or a CI job — anywhere being certain matters more than
//! being brief. The sibling crates in this suite (`audio-helper-rs`,
//! `youtube-helper-rs`, ...) follow the same rule from the start.
//!
//! Both subcommands delegate to the same library calls the older binaries use,
//! so there is one implementation and no second behaviour to keep in step.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Markdown → DOCX and PPTX, pure Rust: no Pandoc, no LaTeX, nothing shelled out.
#[derive(Parser, Debug)]
#[command(name = "md2star-rs", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Convert Markdown to a Word document. Same as the `md2docx` binary.
    Docx {
        /// The input Markdown file (e.g. `report.md`).
        input: PathBuf,
        /// Where to write the `.docx` (defaults to the input name with a `.docx` extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Style the output after an existing `.docx` (Pandoc's `--reference-doc`): its
        /// styles, theme, fonts and page setup are inherited.
        #[arg(long, value_name = "TEMPLATE.docx")]
        reference_doc: Option<PathBuf>,
    },
    /// Convert Markdown to a slide deck. Same as the `md2pptx` binary.
    Pptx {
        /// The input Markdown file. Each `#` heading becomes a slide.
        input: PathBuf,
        /// Where to write the `.pptx` (defaults to the input name with a `.pptx` extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Each arm defaults the output beside the input, then makes the one library
    // call the dedicated binaries already make.
    let (result, output) = match cli.command {
        Command::Docx {
            input,
            output,
            reference_doc,
        } => {
            let output = output.unwrap_or_else(|| input.with_extension("docx"));
            let result = match &reference_doc {
                Some(reference) => {
                    md2star_rs::convert_path_with_reference(&input, &output, reference)
                }
                None => md2star_rs::convert_path(&input, &output),
            };
            (result, output)
        }
        Command::Pptx { input, output } => {
            let output = output.unwrap_or_else(|| input.with_extension("pptx"));
            (md2star_rs::convert_path_to_pptx(&input, &output), output)
        }
    };

    match result {
        Ok(()) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("md2star-rs: {err}");
            ExitCode::FAILURE
        }
    }
}
