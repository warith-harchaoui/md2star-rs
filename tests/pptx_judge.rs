//! The near-tie arbiter: a [`LayoutJudge`] is asked only between layouts that fit
//! equally well, its answer is checked, and a bad answer leaves the deterministic
//! choice. With the `llm` feature, the OpenAI-compatible client is exercised against
//! a fake server on localhost: request shape, verdict, cache.

mod pptx_common;

use std::cell::RefCell;
use std::io::{Cursor, Read, Write};

use md2star_rs::pptx::judge::{Choice, LayoutFacts, LayoutJudge, SlideFacts};
use md2star_rs::{reader, Options};
use pptx_common::Deck;

/// The bundled template plus an exact copy of "Title and Content" named
/// "Title and Content B": two layouts no geometry can tell apart.
pub fn twin_template() -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(Cursor::new(md2star_rs::templates::DEFAULT_PPTX)).unwrap();
    let mut parts = Vec::new();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        parts.push((f.name().to_string(), data));
    }
    let get = |n: &str| {
        parts
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| String::from_utf8(v.clone()).unwrap())
            .unwrap()
    };
    let layout = get("ppt/slideLayouts/slideLayout2.xml").replace(
        "<p:cSld name=\"Title and Content\"",
        "<p:cSld name=\"Title and Content B\"",
    );
    let rels = get("ppt/slideLayouts/_rels/slideLayout2.xml.rels");
    let master = get("ppt/slideMasters/slideMaster1.xml").replace(
        "</p:sldLayoutIdLst>",
        "<p:sldLayoutId id=\"2147483700\" r:id=\"rId99\"/></p:sldLayoutIdLst>",
    );
    let master_rels = get("ppt/slideMasters/_rels/slideMaster1.xml.rels").replace(
        "</Relationships>",
        "<Relationship Id=\"rId99\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout15.xml\"/></Relationships>",
    );
    let ct = get("[Content_Types].xml").replace(
        "</Types>",
        "<Override PartName=\"/ppt/slideLayouts/slideLayout15.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/></Types>",
    );
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in &parts {
        let data = match name.as_str() {
            "ppt/slideMasters/slideMaster1.xml" => master.clone().into_bytes(),
            "ppt/slideMasters/_rels/slideMaster1.xml.rels" => master_rels.clone().into_bytes(),
            "[Content_Types].xml" => ct.clone().into_bytes(),
            _ => data.clone(),
        };
        out.start_file(name.as_str(), opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.start_file("ppt/slideLayouts/slideLayout15.xml", opts)
        .unwrap();
    out.write_all(layout.as_bytes()).unwrap();
    out.start_file("ppt/slideLayouts/_rels/slideLayout15.xml.rels", opts)
        .unwrap();
    out.write_all(rels.as_bytes()).unwrap();
    out.finish().unwrap().into_inner()
}

const MD: &str = "# Deck\n\n## Points\n\n- one\n- two\n- three\n\n## Pinned {layout=\"Two Content\"}\n\n- x\n\n# End\n";

struct Recording {
    answer: Option<&'static str>,
    asked: RefCell<Vec<(SlideFacts, Vec<LayoutFacts>)>>,
}

impl LayoutJudge for Recording {
    fn choose(&self, slide: &SlideFacts, candidates: &[LayoutFacts]) -> Option<Choice> {
        self.asked
            .borrow_mut()
            .push((slide.clone(), candidates.to_vec()));
        self.answer.map(|a| Choice {
            layout: a.into(),
            reason: "test".into(),
        })
    }
}

fn render(judge: &Recording) -> Deck {
    let mut o = Options::default();
    o.reference_doc = Some(twin_template());
    let doc = reader::read(MD);
    let out = md2star_rs::pptx::render_with_judge(&doc, &o, Some(judge)).unwrap();
    Deck::open(&out.bytes)
}

#[test]
fn without_a_judge_the_canonical_layout_wins_the_tie() {
    let mut o = Options::default();
    o.reference_doc = Some(twin_template());
    let out = md2star_rs::convert(MD, md2star_rs::Format::Pptx, &o).unwrap();
    assert_eq!(
        Deck::open(&out.bytes).layout_names()[1],
        "Title and Content"
    );
}

#[test]
fn the_judge_breaks_a_near_tie_and_is_told_facts_in_words() {
    let judge = Recording {
        answer: Some("Title and Content B"),
        asked: RefCell::new(Vec::new()),
    };
    let deck = render(&judge);
    assert_eq!(deck.layout_names()[1], "Title and Content B");
    let asked = judge.asked.borrow();
    // Only the ranked slide was asked: not the cover, not the pinned slide, not the
    // closing.
    assert_eq!(asked.len(), 1, "{asked:?}");
    let (slide, candidates) = &asked[0];
    assert_eq!(
        (slide.index, slide.total, slide.title.as_str()),
        (2, 4, "Points")
    );
    assert!(slide.description.contains("3 paragraph(s) or bullet(s)"));
    let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Title and Content") && names.contains(&"Title and Content B"));
    for c in candidates {
        // Words, not coordinates.
        assert!(c.description.contains("text box"), "{}", c.description);
        assert!(!c
            .description
            .chars()
            .any(|ch| ch.is_ascii_digit() && c.description.contains("EMU")));
    }
}

#[test]
fn an_answer_outside_the_candidates_changes_nothing() {
    let judge = Recording {
        answer: Some("Big Number"),
        asked: RefCell::new(Vec::new()),
    };
    assert_eq!(render(&judge).layout_names()[1], "Title and Content");
    let silent = Recording {
        answer: None,
        asked: RefCell::new(Vec::new()),
    };
    assert_eq!(render(&silent).layout_names()[1], "Title and Content");
}

#[cfg(feature = "llm")]
mod http {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A one-route OpenAI-compatible server: records each request body and answers
    /// with `content` as the assistant's message.
    fn serve(content: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).unwrap();
                log.lock().unwrap().push(String::from_utf8(body).unwrap());
                let answer = serde_json::json!({
                    "choices": [{"message": {"role": "assistant", "content": content}}]
                })
                .to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (url, seen)
    }

    #[test]
    fn smart_layout_asks_the_endpoint_validates_and_caches() {
        let cache = tempfile::tempdir().unwrap();
        // The only environment this binary sets, before any render.
        std::env::set_var("MD2STAR_CACHE_DIR", cache.path());
        let (url, seen) = serve(r#"{"layout": "Title and Content B", "reason": "variety"}"#);
        let mut o = Options::default();
        o.reference_doc = Some(twin_template());
        o.smart_layout = true;
        o.llm_endpoint = Some(url.clone());
        let out = md2star_rs::convert(MD, md2star_rs::Format::Pptx, &o).unwrap();
        assert_eq!(
            Deck::open(&out.bytes).layout_names()[1],
            "Title and Content B"
        );
        let requests = seen.lock().unwrap().clone();
        assert_eq!(requests.len(), 1, "one near-tie, one question");
        let body: serde_json::Value = serde_json::from_str(&requests[0]).unwrap();
        assert_eq!(body["model"], "best");
        assert_eq!(body["response_format"]["type"], "json_schema");
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("Title and Content B"));
        // The verdict is cached on disk: the same deck asks nothing the second time.
        let again = md2star_rs::convert(MD, md2star_rs::Format::Pptx, &o).unwrap();
        assert_eq!(again.bytes, out.bytes);
        assert_eq!(seen.lock().unwrap().len(), 1);
        let cached: Vec<_> = std::fs::read_dir(cache.path().join("layout-verdicts"))
            .unwrap()
            .collect();
        assert_eq!(cached.len(), 1);

        // A model answering nonsense: a warning, and the deterministic choice.
        let (bad, _) = serve("I would rather not say.");
        o.llm_endpoint = Some(bad);
        o.llm_model = Some("other".into());
        let out = md2star_rs::convert(MD, md2star_rs::Format::Pptx, &o).unwrap();
        assert_eq!(
            Deck::open(&out.bytes).layout_names()[1],
            "Title and Content"
        );
        assert!(out.warnings.iter().any(|w| w.code == "smart-layout-failed"));

        // No endpoint at all: a warning, never an error.
        std::env::remove_var("MD2STAR_LLM_ENDPOINT");
        o.llm_endpoint = None;
        let out = md2star_rs::convert(MD, md2star_rs::Format::Pptx, &o).unwrap();
        assert!(out
            .warnings
            .iter()
            .any(|w| w.code == "smart-layout-unavailable"));
    }
}
