//! Semantic line breaking: md2star-rs chooses where a slide's lines end.
//!
//! A port of md2star's `linebreak.py`. A renderer wraps greedily, so a line ends
//! wherever the next word stops fitting: on « de », on « the », before a lone « : »,
//! between « 500 » and « 000 ». When the writer knows a box's width and its text's size,
//! it breaks the lines itself with the face's advance widths ([`super::fonts`]) and
//! writes explicit line breaks the renderer keeps. Break points come from dynamic
//! programming over md2star's cost model:
//!
//! * a line does not end on a function word (article, preposition, conjunction,
//!   pronoun, auxiliary — French and English) whenever any other break fits;
//! * a line never starts with `: ; ? ! » % ) …` and a number is never separated from
//!   the number or the unit after it (500 000, 2 min, 30 %);
//! * no single orphan word on the last line;
//! * breaking after punctuation, or before a word that opens a clause (qui, que, pour,
//!   which, when, because), is preferred;
//! * among the permitted breaks, balanced lines win, and each extra line costs.
//!
//! The same [`wrap`] measures how many lines a paragraph needs, so the fit model, the
//! writer and the audit count lines identically.

use super::fonts::{is_nbsp, Measure};

/// Words that announce what follows: a line ending on one reads as cut.
const GLUE_AFTER: &[&str] = &[
    "le", "la", "les", "l'", "l’", "un", "une", "des", "de", "du", "d'", "d’", "au", "aux", "à",
    "a", "en", "et", "ou", "ni", "que", "qu'", "qu’", "qui", "ne", "n'", "n’", "ce", "c'", "c’",
    "cet", "cette", "ces", "son", "sa", "ses", "mon", "ma", "mes", "ton", "ta", "tes", "notre",
    "nos", "votre", "vos", "leur", "leurs", "se", "s'", "s’", "sur", "sous", "dans", "par", "pour",
    "avec", "sans", "chez", "vers", "entre", "est", "sont", "être", "plus", "très", "pas", "y",
    "il", "elle", "on", "ils", "elles", "nous", "vous", "je", "j'", "j’", "the", "an", "of", "to",
    "in", "at", "by", "for", "and", "or", "nor", "with", "from", "as", "is", "are", "was", "be",
    "not", "no", "our", "your", "their", "its", "his", "her", "my", "this", "that", "these",
    "those", "into", "than", "per", "via",
];

/// Words that open a clause: breaking just before one reads naturally.
const BREAK_BEFORE: &[&str] = &[
    "qui", "que", "quand", "pour", "mais", "donc", "car", "si", "comme", "parce", "puis", "alors",
    "avant", "après", "où", "which", "that", "when", "because", "but", "so", "if", "then", "while",
    "where",
];

/// Characters a line must never start with.
const NO_LINE_START: &[char] = &[':', ';', '?', '!', '»', '%', ')', '…'];

/// Units that stay with the number before them.
const UNITS: &[&str] = &[
    "min", "h", "s", "ms", "pt", "kg", "g", "km", "m", "cm", "mm", "ans", "an", "mois", "jours",
    "j", "md", "mds", "m€", "k€", "€", "$", "%", "x", "fois",
];

fn is_number(word: &str) -> bool {
    let core: String = word
        .chars()
        .filter(|c| !is_nbsp(*c) && !matches!(c, ' ' | ',' | '.'))
        .collect();
    !core.is_empty() && core.chars().all(|c| c.is_ascii_digit())
}

/// Cost of ending a line on word `left` and starting the next on `right`.
pub fn cost_break(left: &str, right: &str) -> f32 {
    let lw = left
        .to_lowercase()
        .trim_matches(|c| matches!(c, '«' | '“' | '(' | '"'))
        .to_string();
    let rw = right
        .to_lowercase()
        .trim_matches(|c| matches!(c, '»' | '”' | ')' | '"' | '.' | ',' | ';' | ':' | '!' | '?'))
        .to_string();
    let first = right.chars().next();
    if first.is_some_and(|c| NO_LINE_START.contains(&c)) || matches!(right, "%" | "€" | "$") {
        return 1e6;
    }
    if left.ends_with('«') || left.ends_with('“') || matches!(left, "•" | "·") {
        return 1e6;
    }
    if is_number(left)
        && (first.is_some_and(|c| c.is_ascii_digit()) || UNITS.contains(&rw.as_str()))
    {
        return 1e6;
    }
    // "1." or "2)" opening a numbered title stays with what it numbers.
    let ordinal = left.len() <= 4
        && left.ends_with(['.', ')'])
        && is_number(left.trim_end_matches(['.', ')']));
    if ordinal || GLUE_AFTER.contains(&lw.as_str()) || lw.ends_with('\'') || lw.ends_with('’') {
        return 400.0;
    }
    if left.ends_with([',', ';', ':', '.', '!', '?', ')', '»', '…']) {
        return -40.0;
    }
    if BREAK_BEFORE.contains(&rw.as_str()) {
        return -25.0;
    }
    0.0
}

/// One word and the spaces after it, with its byte offset in the paragraph.
#[derive(Debug, Clone)]
struct Token<'a> {
    word: &'a str,
    sep: &'a str,
    start: usize,
}

fn tokens(para: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let bytes: Vec<(usize, char)> = para.char_indices().collect();
    let mut i = 0;
    while i < bytes.len() {
        // Skip leading spaces (kept as the paragraph's lead by the caller).
        let start = bytes[i].0;
        let mut j = i;
        while j < bytes.len() && bytes[j].1 != ' ' {
            j += 1;
        }
        let word_end = bytes.get(j).map_or(para.len(), |b| b.0);
        let mut k = j;
        while k < bytes.len() && bytes[k].1 == ' ' {
            k += 1;
        }
        let sep_end = bytes.get(k).map_or(para.len(), |b| b.0);
        if word_end > start {
            out.push(Token {
                word: &para[start..word_end],
                sep: &para[word_end..sep_end],
                start,
            });
        }
        i = k.max(i + 1);
    }
    out
}

/// The byte offsets, in `para`, where each line after the first starts — the breaks
/// [`wrap_para`] chose. Empty when the paragraph fits on one line.
pub fn break_offsets(para: &str, size: f32, max_w: f32, measure: &Measure) -> Vec<usize> {
    let lead_len = para.len() - para.trim_start_matches(' ').len();
    let body = &para[lead_len..];
    let toks = tokens(body);
    let n = toks.len();
    if n <= 1 || measure.width(para.trim_end(), size) <= max_w {
        return Vec::new();
    }
    let line = |i: usize, j: usize| -> String {
        let mut s = String::new();
        if i == 0 {
            s.push_str(&para[..lead_len]);
        }
        for t in &toks[i..j] {
            s.push_str(t.word);
            s.push_str(t.sep);
        }
        s.trim_end_matches(' ').to_string()
    };
    // best[i] = (cost, next break) for the words i.., solved right to left.
    let mut best: Vec<Option<(f32, usize)>> = vec![None; n + 1];
    best[n] = Some((0.0, n));
    for i in (0..n).rev() {
        for j in i + 1..=n {
            let lw = measure.width(&line(i, j), size);
            if lw > max_w && j > i + 1 {
                break;
            }
            let Some((rest, _)) = best[j] else { continue };
            let slack = ((max_w - lw) / max_w).max(0.0);
            let last = j == n;
            let mut cost = if last { 0.0 } else { 100.0 * slack * slack };
            if !last {
                cost += cost_break(toks[j - 1].word, toks[j].word);
            }
            if last && j - i == 1 && n > 2 {
                cost += 300.0;
            }
            cost += 30.0;
            let total = cost + rest;
            if best[i].is_none_or(|(c, _)| total < c) {
                best[i] = Some((total, j));
            }
            if lw > max_w {
                break;
            }
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let Some((_, j)) = best[i] else { break };
        if j < n {
            out.push(lead_len + toks[j].start);
        }
        if j <= i {
            break;
        }
        i = j;
    }
    out
}

/// `text` broken into lines no wider than `max_w` points at `size` points. Existing
/// newlines are kept; a word wider than the box gets a line of its own.
pub fn wrap(text: &str, size: f32, max_w: f32, measure: &Measure) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        out.extend(wrap_para(para, size, max_w, measure));
    }
    out
}

/// One paragraph (no newline) broken into lines.
pub fn wrap_para(para: &str, size: f32, max_w: f32, measure: &Measure) -> Vec<String> {
    let breaks = break_offsets(para, size, max_w, measure);
    if breaks.is_empty() {
        return vec![para.trim_end().to_string()];
    }
    let mut lines = Vec::new();
    let mut from = 0;
    for b in breaks.iter().copied().chain(std::iter::once(para.len())) {
        lines.push(para[from..b].trim_end().to_string());
        from = b;
    }
    lines
}

/// How many lines `text` takes (at least one).
pub fn line_count(text: &str, size: f32, max_w: f32, measure: &Measure) -> usize {
    wrap(text, size, max_w, measure).len().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback() -> Measure {
        Measure::default()
    }

    #[test]
    fn a_short_line_is_left_alone() {
        assert_eq!(
            wrap("Simplicity scales.", 30.0, 1000.0, &fallback()),
            vec!["Simplicity scales."]
        );
    }

    #[test]
    fn a_line_never_ends_on_a_function_word_when_another_break_fits() {
        // ~0.567 em per character at 10 pt: 120 pt holds about 21 characters.
        let lines = wrap(
            "la qualité de la relation compte plus que tout",
            10.0,
            120.0,
            &fallback(),
        );
        assert!(lines.len() > 1, "{lines:?}");
        for line in &lines[..lines.len() - 1] {
            let last = line.rsplit(' ').next().unwrap();
            assert!(!GLUE_AFTER.contains(&last), "{lines:?}");
        }
    }

    #[test]
    fn a_number_stays_with_its_unit() {
        let lines = wrap(
            "nous avons gagné 500 000 clients en 2 min",
            10.0,
            100.0,
            &fallback(),
        );
        assert!(lines.iter().all(|l| !l.starts_with("000")), "{lines:?}");
        assert!(lines.iter().all(|l| !l.starts_with("min")), "{lines:?}");
    }

    #[test]
    fn nothing_is_lost_and_newlines_are_kept() {
        let text = "one two three four five six seven eight nine ten\nsecond paragraph";
        let lines = wrap(text, 12.0, 80.0, &fallback());
        let joined: Vec<&str> = lines
            .iter()
            .flat_map(|l| l.split(' '))
            .filter(|w| !w.is_empty())
            .collect();
        let expected: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(joined, expected);
        assert_eq!(line_count("a\nb", 10.0, 1000.0, &fallback()), 2);
    }

    #[test]
    fn a_colon_never_opens_a_line() {
        assert_eq!(cost_break("mot", ":"), 1e6);
        assert_eq!(cost_break("de", "la"), 400.0);
        assert_eq!(cost_break("fin.", "Puis"), -40.0);
        assert_eq!(cost_break("x", "which"), -25.0);
        assert_eq!(cost_break("1.", "Local"), 400.0);
    }

    #[test]
    fn break_offsets_point_at_word_starts() {
        let para = "alpha beta gamma delta epsilon zeta eta theta";
        let offsets = break_offsets(para, 10.0, 60.0, &fallback());
        assert!(!offsets.is_empty());
        for o in offsets {
            assert_ne!(&para[o..o + 1], " ");
            assert_eq!(&para[o - 1..o], " ");
        }
    }
}
