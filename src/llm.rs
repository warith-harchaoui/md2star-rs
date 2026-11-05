//! A language model behind an OpenAI-compatible endpoint (feature `llm`).
//!
//! One narrow client: a chat completion against `{base}/chat/completions` —
//! best-engine-ai-helper serves one locally at `/v1`, with `model = "best"` resolved per
//! request — never a hard-coded model or vendor. Every call is bounded in time, asks
//! for a JSON answer constrained by a schema (`response_format`), and fails as a clean
//! [`Error::Llm`] so the caller falls back to its deterministic choice.
//!
//! [`HttpJudge`] is the [`LayoutJudge`] `--smart-layout` uses: it hands the model the
//! slide and the near-tied layouts **as words** (computed by the deterministic
//! selector), asks for `{"layout", "reason"}`, and returns nothing on any failure. The
//! selector validates the answer against the candidates and caches it.
//!
//! The endpoint comes from [`crate::Options::llm_endpoint`], else the
//! `MD2STAR_LLM_ENDPOINT` environment variable; the model from
//! [`crate::Options::llm_model`], else `MD2STAR_LLM_MODEL`, else `best`; an API key, when
//! the endpoint wants one, from `MD2STAR_LLM_API_KEY`.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::ast::Warning;
use crate::error::{Error, Result};
use crate::options::Options;
use crate::pptx::judge::{Choice, LayoutFacts, LayoutJudge, SlideFacts};

/// Where to send chat requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The base URL, up to and including `/v1`.
    pub base_url: String,
    /// The model name (`best` lets best-engine-ai-helper choose).
    pub model: String,
    /// An API key, when the endpoint wants one (`MD2STAR_LLM_API_KEY`).
    pub api_key: Option<String>,
    /// The bound on one whole request.
    pub timeout: Duration,
}

impl Endpoint {
    /// The endpoint `options` (or the environment) designate, if any.
    pub fn from_options(options: &Options) -> Option<Self> {
        let base_url = options
            .llm_endpoint
            .clone()
            .or_else(|| std::env::var("MD2STAR_LLM_ENDPOINT").ok())
            .filter(|u| !u.trim().is_empty())?;
        Some(Endpoint {
            base_url,
            model: options
                .llm_model
                .clone()
                .or_else(|| std::env::var("MD2STAR_LLM_MODEL").ok())
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "best".into()),
            api_key: std::env::var("MD2STAR_LLM_API_KEY").ok(),
            timeout: Duration::from_secs(90),
        })
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn post(&self, body: &Value) -> Result<Value> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.post(self.url()).content_type("application/json");
        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let mut response = request
            .send(body.to_string())
            .map_err(|e| Error::Llm(format!("{}: {e}", self.url())))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Llm(format!("unreadable answer: {e}")))?;
        if status >= 400 {
            return Err(Error::Llm(format!(
                "HTTP {status}: {}",
                text.chars().take(200).collect::<String>()
            )));
        }
        serde_json::from_str(&text).map_err(|e| Error::Llm(format!("answer is not JSON: {e}")))
    }

    /// One chat completion: a system prompt and a user message in, the reply's text out.
    ///
    /// # Errors
    ///
    /// [`Error::Llm`] on a transport error, an HTTP error status or an answer with no
    /// message.
    pub fn chat(&self, system: &str, user: &str) -> Result<String> {
        self.complete(system, user, None)
    }

    /// One chat completion whose answer is a JSON object matching `schema`.
    ///
    /// The schema is sent as `response_format` (`json_schema`); an endpoint that refuses
    /// it is asked again with `json_object`. The reply is parsed, not trusted: the caller
    /// still validates its content.
    ///
    /// # Errors
    ///
    /// [`Error::Llm`] when the endpoint fails or the reply is not a JSON object.
    pub fn chat_json(&self, system: &str, user: &str, schema: &Value) -> Result<Value> {
        let format = json!({
            "type": "json_schema",
            "json_schema": {"name": "answer", "schema": schema, "strict": true}
        });
        let text = match self.complete(system, user, Some(format)) {
            Ok(t) => t,
            Err(_) => self.complete(system, user, Some(json!({"type": "json_object"})))?,
        };
        parse_object(&text)
    }

    fn complete(&self, system: &str, user: &str, format: Option<Value>) -> Result<String> {
        let mut body = json!({
            "model": self.model,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });
        if let Some(f) = format {
            body["response_format"] = f;
        }
        let answer = self.post(&body)?;
        answer["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Llm("the answer carries no message".into()))
    }
}

/// The first JSON object in `text` (models wrap JSON in prose or code fences).
fn parse_object(text: &str) -> Result<Value> {
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(text.trim()) {
        return Ok(v);
    }
    let start = text.find('{');
    let end = text.rfind('}');
    if let (Some(s), Some(e)) = (start, end) {
        if s < e {
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(&text[s..=e]) {
                return Ok(v);
            }
        }
    }
    Err(Error::Llm(format!(
        "the reply is not a JSON object: {}",
        text.chars().take(120).collect::<String>()
    )))
}

const JUDGE_SYSTEM: &str = "You choose the layout of one slide in a presentation. \
The candidate layouts all hold the slide's content; pick the one whose design carries \
its role best. Answer with JSON: {\"layout\": one candidate name, copied exactly, \
\"reason\": at most twelve words}.";

/// `--smart-layout`'s judge: an OpenAI-compatible endpoint asked about near-ties.
#[derive(Debug)]
pub struct HttpJudge {
    endpoint: Endpoint,
    warnings: Mutex<Vec<Warning>>,
}

impl HttpJudge {
    /// A judge on `endpoint`.
    pub fn new(endpoint: Endpoint) -> Self {
        HttpJudge {
            endpoint,
            warnings: Mutex::new(Vec::new()),
        }
    }

    /// What went wrong while judging (each failure fell back to the deterministic
    /// choice); drained.
    pub fn take_warnings(&self) -> Vec<Warning> {
        self.warnings
            .lock()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default()
    }

    /// The user message: the slide and its candidates, in words.
    pub fn prompt(slide: &SlideFacts, candidates: &[LayoutFacts]) -> String {
        let position = if slide.index == 1 {
            "the first slide".to_string()
        } else if slide.index == slide.total {
            "the last slide".to_string()
        } else {
            format!("slide {} of {}", slide.index, slide.total)
        };
        let mut s = format!(
            "The slide is {position}, titled \u{201c}{}\u{201d}. It carries {}.\n\nCandidate layouts:\n",
            slide.title, slide.description
        );
        for c in candidates {
            s.push_str(&format!("- {}: {}\n", c.name, c.description));
        }
        s
    }
}

impl LayoutJudge for HttpJudge {
    fn choose(&self, slide: &SlideFacts, candidates: &[LayoutFacts]) -> Option<Choice> {
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let schema = json!({
            "type": "object",
            "properties": {
                "layout": {"type": "string", "enum": names},
                "reason": {"type": "string"}
            },
            "required": ["layout", "reason"],
            "additionalProperties": false
        });
        match self
            .endpoint
            .chat_json(JUDGE_SYSTEM, &Self::prompt(slide, candidates), &schema)
        {
            Ok(v) => Some(Choice {
                layout: v["layout"].as_str()?.to_string(),
                reason: v["reason"].as_str().unwrap_or_default().to_string(),
            }),
            Err(e) => {
                if let Ok(mut w) = self.warnings.lock() {
                    w.push(Warning::new(
                        "smart-layout-failed",
                        format!(
                            "slide {}: the language model was not usable ({e}); the deterministic layout stands",
                            slide.index
                        ),
                    ));
                }
                None
            }
        }
    }

    fn model(&self) -> String {
        format!("{}@{}", self.endpoint.model, self.endpoint.base_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_found_inside_prose_and_fences() {
        let v =
            parse_object("Sure:\n```json\n{\"layout\": \"A\", \"reason\": \"b\"}\n```").unwrap();
        assert_eq!(v["layout"], "A");
        assert!(parse_object("no json here").is_err());
    }

    #[test]
    fn the_prompt_states_facts_in_words() {
        let p = HttpJudge::prompt(
            &SlideFacts {
                index: 1,
                total: 3,
                title: "Hi".into(),
                description: "two bullets".into(),
            },
            &[LayoutFacts {
                name: "Title and Content".into(),
                description: "one text box".into(),
            }],
        );
        assert!(p.contains("the first slide"));
        assert!(p.contains("- Title and Content: one text box"));
    }

    #[test]
    fn an_endpoint_needs_a_url() {
        let o = Options {
            llm_endpoint: Some("http://127.0.0.1:9/v1/".into()),
            ..Options::default()
        };
        let e = Endpoint::from_options(&o).unwrap();
        assert_eq!(e.url(), "http://127.0.0.1:9/v1/chat/completions");
    }
}
