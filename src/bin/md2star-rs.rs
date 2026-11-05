//! `md2star-rs` — Markdown to DOCX, PPTX and PDF (and back), pure Rust.
//!
//! A thin shell over [`md2star_rs::cli`]: the command tree and every subcommand live in
//! the library so the `md2docx` / `md2pptx` shorthands share one implementation.

fn main() -> std::process::ExitCode {
    md2star_rs::cli::main()
}
