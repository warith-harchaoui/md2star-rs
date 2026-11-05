//! A language model behind an OpenAI-compatible endpoint (feature `llm`).
//!
//! # What this module must do
//!
//! Give `--smart-layout` (and any later model-assisted step) one narrow client: a chat
//! completion against `/v1/chat/completions` — best-engine-ai-helper serves one locally,
//! with `model = "best"` resolved per request — never a hard-coded model or vendor.
//! Bounded timeouts on every call, a JSON-schema'd answer where the caller needs
//! structure, and a clean [`Error::Llm`] on anything unexpected so the caller falls back
//! to its deterministic choice. Facts are handed to the model as words, not raw numbers
//! (compute first, describe, then check the answer).
//!
//! The endpoint comes from [`crate::Options::llm_endpoint`], else the
//! `MD2STAR_LLM_ENDPOINT` environment variable; the model from
//! [`crate::Options::llm_model`], else `best`.
//!
//! Owned by the PPTX port (see [`crate::pptx`]).

use crate::error::{Error, Result};
use crate::options::Options;

/// Where to send chat requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The base URL, up to and including `/v1`.
    pub base_url: String,
    /// The model name (`best` lets best-engine-ai-helper choose).
    pub model: String,
    /// An API key, when the endpoint wants one (`MD2STAR_LLM_API_KEY`).
    pub api_key: Option<String>,
}

impl Endpoint {
    /// The endpoint `options` (or the environment) designate, if any.
    pub fn from_options(options: &Options) -> Option<Self> {
        let base_url = options
            .llm_endpoint
            .clone()
            .or_else(|| std::env::var("MD2STAR_LLM_ENDPOINT").ok())?;
        Some(Endpoint {
            base_url,
            model: options.llm_model.clone().unwrap_or_else(|| "best".into()),
            api_key: std::env::var("MD2STAR_LLM_API_KEY").ok(),
        })
    }

    /// One chat completion: a system prompt and a user message in, the reply's text out.
    ///
    /// # Errors
    ///
    /// Today: always [`Error::Unsupported`] — the client is not ported yet.
    pub fn chat(&self, system: &str, user: &str) -> Result<String> {
        let _ = (system, user);
        Err(Error::unsupported("the language-model client"))
    }
}
