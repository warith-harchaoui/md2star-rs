//! The language-model arbiter: a narrow, replaceable judge of near-ties.
//!
//! Layout selection is deterministic and complete without any model
//! ([`super::layout`]). A model is only ever asked to break a **near-tie** between
//! layouts the geometry already says can carry the slide, and only through the
//! [`LayoutJudge`] trait, whose types are plain strings and integers so an iOS or
//! Android host can implement it across an FFI boundary (Apple Foundation Models,
//! Gemini Nano): the crate embeds no weights.
//!
//! What the judge sees are **facts in words** — "two text boxes side by side, title on
//! top; holds about nine lines at 30 pt" — never raw coordinates: a small model reasons
//! about words, not about EMU. What it answers is checked: a layout that is not among
//! the candidates is discarded and the deterministic choice stands, without an error.
//!
//! Verdicts are cached under a hash of the slide facts, the candidates and the model's
//! name ([`VerdictCache`]), so the same deck asks the same question once and comes out
//! the same on every run — idempotence survives a non-deterministic model.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// What the judge is told about a slide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideFacts {
    /// 1-based position in the deck.
    pub index: u32,
    /// Number of slides in the deck.
    pub total: u32,
    /// The slide's title as printed.
    pub title: String,
    /// What the slide carries, in words.
    pub description: String,
}

/// What the judge is told about one candidate layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutFacts {
    /// The layout's exact name: the answer must be one of these.
    pub name: String,
    /// Its geometry and capacity, in words.
    pub description: String,
}

/// The judge's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The chosen layout's name.
    pub layout: String,
    /// Why, in a few words.
    pub reason: String,
}

/// Something that can break a near-tie between layouts.
pub trait LayoutJudge {
    /// Pick one of `candidates` for `slide`, or `None` to leave the deterministic
    /// choice in place. Any answer naming a layout outside `candidates` is ignored.
    fn choose(&self, slide: &SlideFacts, candidates: &[LayoutFacts]) -> Option<Choice>;

    /// The model's name, part of the cache key: a different model is a different
    /// question.
    fn model(&self) -> String {
        "unnamed".into()
    }
}

/// Keep `choice` only when it names one of `candidates` (exactly, or ignoring case
/// and surrounding spaces), returning it with the canonical name.
pub fn validate(choice: Option<Choice>, candidates: &[LayoutFacts]) -> Option<Choice> {
    let choice = choice?;
    let wanted = choice.layout.trim();
    let hit = candidates.iter().find(|c| c.name == wanted).or_else(|| {
        let folded = wanted.to_lowercase();
        let mut matches = candidates
            .iter()
            .filter(|c| c.name.to_lowercase() == folded);
        let first = matches.next();
        if matches.next().is_some() {
            None
        } else {
            first
        }
    })?;
    Some(Choice {
        layout: hit.name.clone(),
        reason: choice.reason.trim().chars().take(200).collect(),
    })
}

/// A stable 128-bit FNV-1a hash, hex-encoded: the same text gives the same key on every
/// machine and every Rust release (unlike `std`'s `DefaultHasher`).
pub fn stable_hash(text: &str) -> String {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut b: u64 = 0x6c62_272e_07bb_0142;
    for byte in text.bytes() {
        a ^= u64::from(byte);
        a = a.wrapping_mul(0x0000_0100_0000_01b3);
        b ^= u64::from(byte);
        b = b.wrapping_mul(0x0000_0100_0000_01b3);
        b = b.rotate_left(5);
    }
    format!("{a:016x}{b:016x}")
}

/// The cache key of one question.
pub fn cache_key(slide: &SlideFacts, candidates: &[LayoutFacts], model: &str) -> String {
    let mut text = format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{model}",
        slide.index, slide.total, slide.title, slide.description
    );
    for c in candidates {
        text.push('\u{1e}');
        text.push_str(&c.name);
        text.push('\u{1f}');
        text.push_str(&c.description);
    }
    stable_hash(&text)
}

/// Verdicts by question: in memory, and on disk when a directory is given (one small
/// file per verdict: the layout on the first line, the reason on the second; an empty
/// file records "no answer").
#[derive(Debug, Default)]
pub struct VerdictCache {
    memory: Mutex<BTreeMap<String, Option<Choice>>>,
    dir: Option<PathBuf>,
}

impl VerdictCache {
    /// An in-memory cache.
    pub fn in_memory() -> Self {
        VerdictCache::default()
    }

    /// A cache persisted under `dir` (created on first write).
    pub fn on_disk(dir: PathBuf) -> Self {
        VerdictCache {
            memory: Mutex::new(BTreeMap::new()),
            dir: Some(dir),
        }
    }

    /// The default on-disk location: `$MD2STAR_CACHE_DIR/layout-verdicts`, else
    /// `$XDG_CACHE_HOME/md2star-rs/layout-verdicts`, else
    /// `~/.cache/md2star-rs/layout-verdicts`; `None` when no home is known (a phone).
    pub fn default_dir() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("MD2STAR_CACHE_DIR") {
            return Some(PathBuf::from(d).join("layout-verdicts"));
        }
        if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
            return Some(PathBuf::from(d).join("md2star-rs/layout-verdicts"));
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/md2star-rs/layout-verdicts"))
    }

    /// The cached verdict for `key`: `Some(verdict)` when the question was asked before.
    pub fn get(&self, key: &str) -> Option<Option<Choice>> {
        if let Ok(m) = self.memory.lock() {
            if let Some(v) = m.get(key) {
                return Some(v.clone());
            }
        }
        let path = self.dir.as_ref()?.join(key);
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let verdict = lines.next().filter(|l| !l.is_empty()).map(|layout| Choice {
            layout: layout.to_string(),
            reason: lines.next().unwrap_or_default().to_string(),
        });
        if let Ok(mut m) = self.memory.lock() {
            m.insert(key.to_string(), verdict.clone());
        }
        Some(verdict)
    }

    /// Record a verdict.
    pub fn put(&self, key: &str, verdict: Option<Choice>) {
        if let Some(dir) = &self.dir {
            let body = match &verdict {
                Some(c) => format!(
                    "{}\n{}\n",
                    c.layout.replace('\n', " "),
                    c.reason.replace('\n', " ")
                ),
                None => String::new(),
            };
            if std::fs::create_dir_all(dir).is_ok() {
                let _ = std::fs::write(dir.join(key), body);
            }
        }
        if let Ok(mut m) = self.memory.lock() {
            m.insert(key.to_string(), verdict);
        }
    }
}

/// A judge behind a verdict cache, with every answer validated.
pub struct CachedJudge<'a> {
    /// The judge asked on a cache miss.
    pub inner: &'a dyn LayoutJudge,
    /// Where verdicts are kept.
    pub cache: &'a VerdictCache,
}

impl LayoutJudge for CachedJudge<'_> {
    fn choose(&self, slide: &SlideFacts, candidates: &[LayoutFacts]) -> Option<Choice> {
        let key = cache_key(slide, candidates, &self.inner.model());
        if let Some(hit) = self.cache.get(&key) {
            return validate(hit, candidates);
        }
        let verdict = validate(self.inner.choose(slide, candidates), candidates);
        self.cache.put(&key, verdict.clone());
        verdict
    }

    fn model(&self) -> String {
        self.inner.model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn facts() -> (SlideFacts, Vec<LayoutFacts>) {
        (
            SlideFacts {
                index: 2,
                total: 5,
                title: "Why".into(),
                description: "three bullets".into(),
            },
            vec![
                LayoutFacts {
                    name: "Title and Content".into(),
                    description: "one text box".into(),
                },
                LayoutFacts {
                    name: "Two Content".into(),
                    description: "two text boxes".into(),
                },
            ],
        )
    }

    struct Fixed(&'static str, Cell<u32>);
    impl LayoutJudge for Fixed {
        fn choose(&self, _: &SlideFacts, _: &[LayoutFacts]) -> Option<Choice> {
            self.1.set(self.1.get() + 1);
            Some(Choice {
                layout: self.0.into(),
                reason: "because".into(),
            })
        }
        fn model(&self) -> String {
            "fixed".into()
        }
    }

    #[test]
    fn an_answer_outside_the_candidates_is_dropped() {
        let (_, c) = facts();
        let bad = Some(Choice {
            layout: "Big Number".into(),
            reason: String::new(),
        });
        assert_eq!(validate(bad, &c), None);
        let loose = Some(Choice {
            layout: " two content ".into(),
            reason: "x".into(),
        });
        assert_eq!(validate(loose, &c).unwrap().layout, "Two Content");
    }

    #[test]
    fn a_verdict_is_asked_once_and_replayed() {
        let (s, c) = facts();
        let judge = Fixed("Two Content", Cell::new(0));
        let cache = VerdictCache::in_memory();
        let cached = CachedJudge {
            inner: &judge,
            cache: &cache,
        };
        assert_eq!(cached.choose(&s, &c).unwrap().layout, "Two Content");
        assert_eq!(cached.choose(&s, &c).unwrap().layout, "Two Content");
        assert_eq!(judge.1.get(), 1);
    }

    #[test]
    fn the_disk_cache_survives_a_new_process() {
        let dir = std::env::temp_dir().join(format!("md2star-rs-verdicts-{}", std::process::id()));
        let (s, c) = facts();
        let key = cache_key(&s, &c, "m");
        VerdictCache::on_disk(dir.clone()).put(
            &key,
            Some(Choice {
                layout: "Two Content".into(),
                reason: "two ideas".into(),
            }),
        );
        let again = VerdictCache::on_disk(dir.clone());
        assert_eq!(again.get(&key).unwrap().unwrap().reason, "two ideas");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_key_depends_on_the_model() {
        let (s, c) = facts();
        assert_ne!(cache_key(&s, &c, "a"), cache_key(&s, &c, "b"));
        assert_eq!(cache_key(&s, &c, "a"), cache_key(&s, &c, "a"));
        assert_eq!(stable_hash("").len(), 32);
    }
}
