//! `md2pptx` — the command-line front end for [`md2star_rs`]'s PPTX backend.
//!
//! The slide-deck sibling of `md2docx`: parse `input` (+ optional `-o/--output`), default the
//! output name to the input with a `.pptx` extension, and delegate the real work to the library.
//! Each level-1 heading in the Markdown becomes a slide. Errors go to stderr with a non-zero
//! exit; success prints the path.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Markdown → PPTX, pure Rust (no Pandoc). A spin-off of the Pandoc-backed `md2star`.
#[derive(Parser, Debug)]
#[command(name = "md2pptx", version, about)]
struct Cli {
    /// The input Markdown file (e.g. `talk.md`). Each `#` heading becomes a slide.
    input: PathBuf,

    /// Where to write the `.pptx` (defaults to the input name with a `.pptx` extension).
    #[arg(short, long)]
    output: Option<PathBuf>,
}

/// Parse arguments, run the conversion, and map the outcome to a process exit code.
fn main() -> ExitCode {
    let cli = Cli::parse();

    // Default the output next to the input: `talk.md` → `talk.pptx`.
    let output = cli
        .output
        .unwrap_or_else(|| cli.input.with_extension("pptx"));

    // One library call does read → convert → write; we only turn the result into a message.
    match md2star_rs::convert_path_to_pptx(&cli.input, &output) {
        Ok(()) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("md2pptx: {err}");
            ExitCode::FAILURE
        }
    }
}
