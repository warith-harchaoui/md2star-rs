//! Font metrics: how wide a run of text prints, from the face's own advance widths.
//!
//! Fitting text into a placeholder is arithmetic once the type size and the box are
//! known — except for the width of the characters. md2star-rs reads it from the font
//! file itself with `ttf-parser` (pure Rust): the `cmap` and `hmtx` tables give every
//! character's advance in font units. The face is looked up by the family the template
//! names (the theme's heading and body fonts, resolved by [`super::template`]) in the
//! usual font folders, plus `MD2STAR_FONT_DIRS` (a `:`-separated list) first.
//!
//! When the file cannot be found — the common case on a phone, and for a template
//! whose fonts are not installed — the **documented default** applies: every character
//! advances [`FALLBACK_EM`] of the em (0.55, md2star's `linebreak.FALLBACK_EM`), or
//! [`FALLBACK_MONO_EM`] for code. Every measured width is multiplied by
//! [`WIDTH_MARGIN`], so a renderer's kerning or hinting can never re-wrap a line
//! md2star-rs broke itself. The fallback is deliberately on the wide side: an estimate
//! that errs makes a box look fuller than it is, which splits a slide one bullet early
//! rather than letting text spill.
//!
//! The lookup reads the file system only; on a platform without font folders it
//! simply finds nothing and the default applies.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Advance of one character, as a fraction of the em, when the face cannot be read.
pub const FALLBACK_EM: f32 = 0.55;
/// The same for a monospaced face (code).
pub const FALLBACK_MONO_EM: f32 = 0.6;
/// Safety factor on every measured width.
pub const WIDTH_MARGIN: f32 = 1.03;

/// The advance widths of one face.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// Font units per em.
    pub units_per_em: u16,
    /// Advance, in font units, of every character the face maps.
    pub advances: HashMap<char, u16>,
    /// The file it was read from.
    pub path: PathBuf,
}

impl Metrics {
    /// Read the metrics of the first face of `data` (a TTF, OTF or collection).
    pub fn from_bytes(data: &[u8], path: PathBuf) -> Option<Self> {
        let face = ttf_parser::Face::parse(data, 0).ok()?;
        let mut advances = HashMap::new();
        if let Some(cmap) = face.tables().cmap {
            for sub in cmap.subtables {
                if !sub.is_unicode() {
                    continue;
                }
                sub.codepoints(|cp| {
                    if let Some(ch) = char::from_u32(cp) {
                        if let Some(gid) = sub.glyph_index(cp) {
                            if let Some(adv) = face.glyph_hor_advance(gid) {
                                advances.entry(ch).or_insert(adv);
                            }
                        }
                    }
                });
            }
        }
        if advances.is_empty() {
            return None;
        }
        Some(Metrics {
            units_per_em: face.units_per_em().max(1),
            advances,
            path,
        })
    }

    /// Whether the face draws `ch`.
    pub fn has_glyph(&self, ch: char) -> bool {
        self.advances.contains_key(&ch)
    }
}

/// How text is measured: a face's real metrics, or the fallback em.
#[derive(Debug, Clone, Default)]
pub struct Measure {
    /// The face, when its file was found.
    pub metrics: Option<Arc<Metrics>>,
    /// Monospaced fallback (code) rather than proportional.
    pub mono: bool,
}

impl Measure {
    /// The measure for `family` (bold or not), looked up on this machine.
    pub fn for_family(family: Option<&str>, bold: bool, mono: bool) -> Self {
        Measure {
            metrics: family.and_then(|f| face(f, bold)),
            mono,
        }
    }

    /// Printed width of `text` at `size` points, in points.
    pub fn width(&self, text: &str, size: f32) -> f32 {
        let fallback = if self.mono {
            FALLBACK_MONO_EM
        } else {
            FALLBACK_EM
        };
        let em = match &self.metrics {
            None => text.chars().count() as f32 * fallback,
            Some(m) => {
                let upm = f32::from(m.units_per_em);
                text.chars()
                    .map(|ch| {
                        let ch = if is_nbsp(ch) && !m.has_glyph(ch) {
                            ' '
                        } else {
                            ch
                        };
                        m.advances
                            .get(&ch)
                            .map_or(fallback, |a| f32::from(*a) / upm)
                    })
                    .sum()
            }
        };
        em * size * WIDTH_MARGIN
    }

    /// Whether the face is known to lack `ch` (always `false` without metrics).
    pub fn lacks(&self, ch: char) -> bool {
        self.metrics.as_ref().is_some_and(|m| !m.has_glyph(ch))
    }
}

/// The no-break spaces a renderer may refuse to draw.
pub fn is_nbsp(ch: char) -> bool {
    matches!(ch, '\u{a0}' | '\u{202f}' | '\u{2007}')
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var("MD2STAR_FONT_DIRS")
        .map(|v| {
            v.split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    for d in [
        "/Library/Fonts",
        "/System/Library/Fonts",
        "/System/Library/Fonts/Supplemental",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
    ] {
        dirs.push(PathBuf::from(d));
    }
    // Windows keeps its fonts under the system root, wherever it was installed.
    if let Some(root) = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR")) {
        dirs.push(PathBuf::from(root).join("Fonts"));
    }
    dirs
}

/// Every font file under the font folders, by lower-cased file stem (first found wins,
/// in folder order, so `MD2STAR_FONT_DIRS` takes precedence).
fn index() -> &'static BTreeMap<String, PathBuf> {
    static INDEX: OnceLock<BTreeMap<String, PathBuf>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = BTreeMap::new();
        for dir in font_dirs() {
            scan(&dir, 0, &mut map);
        }
        map
    })
}

fn scan(dir: &Path, depth: usize, map: &mut BTreeMap<String, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if depth < 3 {
                scan(&path, depth + 1, map);
            }
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc")) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            map.entry(stem.to_ascii_lowercase()).or_insert(path.clone());
        }
    }
}

/// The candidate file stems for a family in a weight, most specific first.
pub fn candidate_stems(family: &str, bold: bool) -> Vec<String> {
    let squeezed: String = family.chars().filter(|c| !c.is_whitespace()).collect();
    let dashed = family.trim().replace(' ', "-");
    let weights: &[&str] = if bold {
        &["Bold", "SemiBold", "Semibold", "Medium"]
    } else {
        &["Regular", "Book", "Roman", ""]
    };
    let mut out = Vec::new();
    for base in [&squeezed, &dashed, &family.trim().to_string()] {
        for w in weights {
            let stem = if w.is_empty() {
                base.clone()
            } else {
                format!("{base}-{w}")
            };
            let stem = stem.to_ascii_lowercase();
            if !out.contains(&stem) {
                out.push(stem);
            }
        }
    }
    if bold {
        let plain = squeezed.to_ascii_lowercase();
        if !out.contains(&plain) {
            out.push(plain);
        }
    }
    out
}

/// The metrics of `family` in that weight, when its file is on this machine.
pub fn face(family: &str, bold: bool) -> Option<Arc<Metrics>> {
    type Cache = Mutex<HashMap<(String, bool), Option<Arc<Metrics>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (family.to_string(), bold);
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let found = candidate_stems(family, bold).into_iter().find_map(|stem| {
        let path = index().get(&stem)?;
        let data = std::fs::read(path).ok()?;
        Metrics::from_bytes(&data, path.clone()).map(Arc::new)
    });
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, found.clone());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_is_a_fixed_em() {
        let m = Measure::default();
        let w = m.width("abcd", 10.0);
        assert!((w - 4.0 * 0.55 * 10.0 * WIDTH_MARGIN).abs() < 1e-3);
        let mono = Measure {
            mono: true,
            ..Measure::default()
        };
        assert!(mono.width("abcd", 10.0) > w);
    }

    #[test]
    fn stems_cover_the_usual_file_names() {
        let stems = candidate_stems("Open Sans", false);
        assert!(stems.contains(&"opensans-regular".to_string()));
        assert!(stems.contains(&"opensans".to_string()));
        assert!(stems.contains(&"open-sans-regular".to_string()));
        assert!(candidate_stems("Inter", true).contains(&"inter-bold".to_string()));
    }

    #[test]
    fn an_unknown_family_measures_with_the_fallback() {
        let m = Measure::for_family(Some("No Such Family 4f9a"), false, false);
        assert!(m.metrics.is_none());
        assert!(!m.lacks('x'));
    }

    #[test]
    fn a_real_face_is_read_when_one_is_installed() {
        // Whatever the machine has: the first font of the index, if any. On a bare CI
        // image there may be none, which is exactly the fallback path tested above.
        let Some(path) = index().values().next() else {
            return;
        };
        let data = std::fs::read(path).unwrap();
        if let Some(m) = Metrics::from_bytes(&data, path.clone()) {
            assert!(m.units_per_em > 0);
            assert!(!m.advances.is_empty());
        }
    }
}
