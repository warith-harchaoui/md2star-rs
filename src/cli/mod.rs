//! The command-line front end (feature `cli`): `md2star-rs <command>`, plus the
//! `md2docx` / `md2pptx` shorthands.
//!
//! Ownership: this file (the command tree, the options every conversion shares, the
//! read → convert → write → report loop) is the shared foundation. Each subcommand's own
//! flags and `run` live in its own file, owned by the port that implements it:
//! `docx.rs` (DOCX port), `pptx.rs` / `audit.rs` / `templates.rs` (PPTX port),
//! `pdf.rs` / `twin.rs` (twin + PDF port).
//!
//! Options are aligned on the Python `md2star <cmd> --help` where they mean something
//! without Pandoc. Pandoc pass-through flags, `--skip-phase`, `--lint` and
//! `--no-remote-templates` do not apply to an in-process converter with no Pandoc and
//! no remote templates, and are not offered.
//!
//! The library never prints; this module does. Warnings go to stderr unless `--quiet`,
//! the written path to stdout, and an error ends the process with status 1 after one
//! `md2star-rs: …` line.

pub mod audit;
pub mod docx;
pub mod pdf;
pub mod pptx;
pub mod templates;
pub mod twin;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::ast::Warning;
use crate::error::Result;
use crate::options::{Format, Options};

/// Markdown → DOCX and PPTX, pure Rust: no Pandoc, no LibreOffice, nothing shelled
/// out. PDF, twin and audit are being ported and say so when run.
#[derive(Parser, Debug)]
#[command(name = "md2star-rs", version)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Markdown → Microsoft Word (.docx). Same as the `md2docx` binary.
    Docx(docx::DocxArgs),
    /// Markdown → Microsoft PowerPoint (.pptx). Same as the `md2pptx` binary.
    Pptx(pptx::PptxArgs),
    /// Markdown → PDF, in process (needs the `pdf` feature).
    Pdf(pdf::PdfArgs),
    /// Read a .docx or .pptx back into an editable Markdown twin.
    Twin(twin::TwinArgs),
    /// Report overlaps, off-canvas shapes and unreadable type in any .pptx.
    Audit(audit::AuditArgs),
    /// Inspect the templates compiled into this binary.
    Templates(templates::TemplatesArgs),
}

/// `-v` / `-q`, shared by every command.
#[derive(Args, Debug, Clone, Copy, Default)]
pub struct Verbosity {
    /// Show debug-level diagnostics on stderr.
    #[arg(short, long, conflicts_with = "quiet")]
    pub verbose: bool,
    /// Suppress info and warnings; errors only.
    #[arg(short, long)]
    pub quiet: bool,
}

/// The flags every conversion (`docx`, `pptx`, `pdf`) shares.
#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// The input Markdown file.
    #[arg(value_name = "INPUT.md")]
    pub input: PathBuf,
    /// Output path (default: the input with the format's extension).
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Document author (overrides the front matter).
    #[arg(long)]
    pub author: Option<String>,
    /// Bibliography file (BibTeX). Repeatable.
    #[arg(long, value_name = "FILE")]
    pub bib: Vec<PathBuf>,
    /// Heading inserted before the bibliography (default: localised "References").
    #[arg(long, value_name = "TEXT")]
    pub bibliography_name: Option<String>,
    /// Citation style: `author-date` (default) or `numeric`.
    #[arg(long, value_name = "STYLE")]
    pub citation_style: Option<String>,
    /// Document language (BCP 47, e.g. en-US).
    #[arg(long)]
    pub lang: Option<String>,
    /// Date shown under the title, used verbatim (e.g. '21 juin 2026', '2026-Q2').
    #[arg(long)]
    pub date: Option<String>,
    /// Template whose styles (DOCX/PDF) or masters and layouts (PPTX) the output inherits.
    #[arg(long, value_name = "PATH")]
    pub reference_doc: Option<PathBuf>,
    /// Insert a table of contents.
    #[arg(long)]
    pub toc: bool,
    /// Forbid every network access (overrides --allow-remote-images).
    #[arg(long)]
    pub offline: bool,
    /// Download ![](https://...) images referenced in the Markdown (needs the `net`
    /// feature). Off by default.
    #[arg(long)]
    pub allow_remote_images: bool,
    /// -v / -q.
    #[command(flatten)]
    pub verbosity: Verbosity,
}

impl ConvertArgs {
    /// The [`Options`] these flags describe (the reference document read from disk;
    /// relative resources resolved against the input's directory).
    ///
    /// # Errors
    ///
    /// [`crate::Error::Io`] when `--reference-doc` cannot be read.
    pub fn options(&self) -> Result<Options> {
        let reference_doc = match &self.reference_doc {
            Some(path) => Some(fs::read(path)?),
            None => None,
        };
        Ok(Options {
            base_dir: Some(
                self.input
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
            ),
            author: self.author.clone(),
            date: self.date.clone(),
            lang: self.lang.clone(),
            bibliography: self.bib.clone(),
            bibliography_name: self.bibliography_name.clone(),
            citation_style: self.citation_style.clone(),
            reference_doc,
            toc: self.toc,
            offline: self.offline,
            allow_remote_images: self.allow_remote_images,
            ..Options::default()
        })
    }

    /// Where the output goes: `-o`, or the input with `format`'s extension.
    pub fn output_path(&self, format: Format) -> PathBuf {
        self.output
            .clone()
            .unwrap_or_else(|| self.input.with_extension(format.extension()))
    }
}

/// Read `args.input`, convert it to `format` with `options`, write the result, report.
///
/// # Errors
///
/// Reading, converting or writing failed.
pub fn convert_file(args: &ConvertArgs, format: Format, options: &Options) -> Result<()> {
    let markdown = fs::read_to_string(&args.input)?;
    let rendered = crate::convert(&markdown, format, options)?;
    let output = args.output_path(format);
    fs::write(&output, &rendered.bytes)?;
    report_warnings(&rendered.warnings, args.verbosity);
    if !args.verbosity.quiet {
        println!("wrote {}", output.display());
    }
    Ok(())
}

/// Print warnings on stderr (unless quiet); with `-v`, prefix each with its code.
pub fn report_warnings(warnings: &[Warning], verbosity: Verbosity) {
    if verbosity.quiet {
        return;
    }
    for w in warnings {
        if verbosity.verbose {
            eprintln!("md2star-rs: warning [{}]: {}", w.code, w.message);
        } else {
            eprintln!("md2star-rs: warning: {}", w.message);
        }
    }
}

/// Turn a command's result into an exit status, printing the error if any.
fn finish(program: &str, result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{program}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Run one subcommand.
///
/// # Errors
///
/// Whatever the subcommand reports.
pub fn run(command: &Command) -> Result<()> {
    match command {
        Command::Docx(args) => docx::run(args),
        Command::Pptx(args) => pptx::run(args),
        Command::Pdf(args) => pdf::run(args),
        Command::Twin(args) => twin::run(args),
        Command::Audit(args) => audit::run(args),
        Command::Templates(args) => templates::run(args),
    }
}

/// `main` of the `md2star-rs` binary.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    finish("md2star-rs", run(&cli.command))
}

/// `main` of the `md2docx` shorthand: the `docx` subcommand under its own name.
pub fn md2docx_main() -> ExitCode {
    /// Markdown → DOCX, pure Rust (no Pandoc). Same as `md2star-rs docx`.
    #[derive(Parser, Debug)]
    #[command(name = "md2docx", version)]
    struct Md2Docx {
        #[command(flatten)]
        args: docx::DocxArgs,
    }
    let cli = Md2Docx::parse();
    finish("md2docx", docx::run(&cli.args))
}

/// `main` of the `md2pptx` shorthand: the `pptx` subcommand under its own name.
pub fn md2pptx_main() -> ExitCode {
    /// Markdown → PPTX, pure Rust (no Pandoc). Same as `md2star-rs pptx`.
    #[derive(Parser, Debug)]
    #[command(name = "md2pptx", version)]
    struct Md2Pptx {
        #[command(flatten)]
        args: pptx::PptxArgs,
    }
    let cli = Md2Pptx::parse();
    finish("md2pptx", pptx::run(&cli.args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn docx_flags_parse() {
        let cli = Cli::try_parse_from([
            "md2star-rs",
            "docx",
            "in.md",
            "-o",
            "out.docx",
            "--bib",
            "a.bib",
            "--bib",
            "b.bib",
            "--lang",
            "fr-FR",
            "--toc",
            "-q",
        ])
        .unwrap();
        let Command::Docx(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.common.bib.len(), 2);
        assert!(args.common.toc && args.common.verbosity.quiet);
        assert_eq!(
            args.common.output_path(Format::Docx),
            PathBuf::from("out.docx")
        );
    }

    #[test]
    fn output_defaults_beside_the_input() {
        let cli = Cli::try_parse_from(["md2star-rs", "pptx", "talks/deck.md"]).unwrap();
        let Command::Pptx(args) = cli.command else {
            panic!()
        };
        assert_eq!(
            args.common.output_path(Format::Pptx),
            PathBuf::from("talks/deck.pptx")
        );
        let options = args.common.options().unwrap();
        assert_eq!(options.base_dir, Some(PathBuf::from("talks")));
    }
}
