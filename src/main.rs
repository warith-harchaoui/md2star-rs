//! `md2docx` — the `docx` subcommand of [`md2star_rs::cli`] under the name the Python
//! md2star made familiar. Same flags as `md2star-rs docx`.

fn main() -> std::process::ExitCode {
    md2star_rs::cli::md2docx_main()
}
