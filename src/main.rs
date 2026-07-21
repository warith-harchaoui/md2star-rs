//! `md2docx` — the command-line front end for [`md2star_rs`].
//!
//! Deliberately thin: parse `input` (+ optional `-o/--output`), default the output name to
//! the input with a `.docx` extension, and delegate the real work to the library. All the
//! Markdown → DOCX logic lives in the crate so the binary, a server, or a WASM shell can
//! share it. Errors are reported to stderr with a non-zero exit; success prints the path.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Markdown → DOCX, pure Rust (no Pandoc). A spin-off of the Pandoc-backed `md2star`.
#[derive(Parser, Debug)]
#[command(name = "md2docx", version, about)]
struct Cli {
    /// The input Markdown file (e.g. `report.md`).
    input: PathBuf,

    /// Where to write the `.docx` (defaults to the input name with a `.docx` extension).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Style the output after an existing `.docx` (Pandoc's `--reference-doc`): its styles,
    /// theme, fonts and page setup are inherited; headings/quotes use its named styles.
    #[arg(long, value_name = "TEMPLATE.docx")]
    reference_doc: Option<PathBuf>,
}

/// Parse arguments, run the conversion, and map the outcome to a process exit code.
fn main() -> ExitCode {
    let cli = Cli::parse();

    // Default the output next to the input: `report.md` → `report.docx`.
    let output = cli
        .output
        .unwrap_or_else(|| cli.input.with_extension("docx"));

    // One call into the library does the read → convert → write; we only translate the
    // result into a user-facing message and an exit code here. With `--reference-doc`, route
    // through the styling-aware variant; otherwise use the plain path.
    let result = match &cli.reference_doc {
        Some(reference) => md2star_rs::convert_path_with_reference(&cli.input, &output, reference),
        None => md2star_rs::convert_path(&cli.input, &output),
    };
    match result {
        Ok(()) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("md2docx: {err}");
            ExitCode::FAILURE
        }
    }
}
