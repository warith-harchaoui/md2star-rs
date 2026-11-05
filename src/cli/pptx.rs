//! `md2star-rs pptx` / `md2pptx` (owned by the PPTX port).

use std::fs;
use std::path::PathBuf;

use clap::Args;

use super::{convert_file, ConvertArgs};
use crate::error::Result;
use crate::options::Format;

/// Flags of `md2star-rs pptx`.
#[derive(Args, Debug, Clone)]
pub struct PptxArgs {
    /// The flags every conversion shares.
    #[command(flatten)]
    pub common: ConvertArgs,
    /// Designer .pptx template whose own named layouts the slides are mapped onto
    /// (same as --reference-doc for a deck; wins over it).
    #[arg(long, value_name = "PATH")]
    pub template: Option<PathBuf>,
    /// Never set slide text under N points (Kawasaki's 10/20/30: pass 30): change the
    /// layout or split the slide instead of shrinking.
    #[arg(long, value_name = "N")]
    pub min_pt: Option<f32>,
    /// Let a language model pick each slide's layout (needs the `llm` feature and an
    /// OpenAI-compatible endpoint); the deterministic choice is the fallback.
    #[arg(long)]
    pub smart_layout: bool,
    /// The OpenAI-compatible endpoint for --smart-layout (default: $MD2STAR_LLM_ENDPOINT).
    #[arg(long, value_name = "URL")]
    pub llm_endpoint: Option<String>,
    /// The model for --smart-layout (default: `best`).
    #[arg(long, value_name = "NAME")]
    pub llm_model: Option<String>,
}

/// Run `pptx`.
///
/// # Errors
///
/// Reading, converting or writing failed.
pub fn run(args: &PptxArgs) -> Result<()> {
    let mut options = args.common.options()?;
    if let Some(template) = &args.template {
        options.reference_doc = Some(fs::read(template)?);
    }
    options.min_pt = args.min_pt;
    options.smart_layout = args.smart_layout;
    options.llm_endpoint = args.llm_endpoint.clone();
    options.llm_model = args.llm_model.clone();
    convert_file(&args.common, Format::Pptx, &options)
}
