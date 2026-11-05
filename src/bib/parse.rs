//! The BibTeX parser (see [`crate::bib`] for what it must accept).
//!
//! Two passes over each entry: the *structure* (`@type{key, field = value, …}`, with
//! `@string` macros, `#` concatenation, quoted or braced values and bare numbers) is read
//! on the raw text; the *values* are then decoded from LaTeX to Unicode
//! ([`decode_latex`]). Name lists (`author`, `editor`) are split on the raw value, before
//! decoding, because a brace-protected `{Barnes and Noble}` is one name and only the raw
//! text still says so.

use std::collections::{BTreeMap, HashMap};

use super::{Bibliography, Entry, Name};
use crate::error::{Error, Result};

/// Parse BibTeX source into a [`Bibliography`].
///
/// `@comment` and `@preamble` blocks are skipped, `@string` macros are expanded (the
/// twelve month macros `jan` … `dec` are predefined), and text outside an entry is
/// ignored, as BibTeX itself does.
///
/// # Errors
///
/// [`Error::Bib`] on input that cannot be read as BibTeX (an unclosed brace or quote, an
/// entry without a key), naming the line where the faulty entry starts.
///
/// # Examples
///
/// ```
/// let bib = md2star_rs::bib::parse(
///     "@string{cup = {Cambridge University Press}}\n\
///      @book{pearl2009, author = {Pearl, Judea}, title = {Causality},\n\
///            publisher = cup, year = 2009}",
/// )
/// .unwrap();
/// let entry = bib.get("pearl2009").unwrap();
/// assert_eq!(entry.field("publisher"), Some("Cambridge University Press"));
/// assert_eq!(entry.authors[0].family, "Pearl");
/// ```
pub fn parse(source: &str) -> Result<Bibliography> {
    let mut parser = Parser {
        src: source.as_bytes(),
        text: source,
        pos: 0,
        macros: month_macros(),
    };
    let mut entries = Vec::new();
    while let Some(at) = parser.find_at() {
        parser.pos = at + 1;
        let line = parser.line_of(at);
        let kind = parser.identifier().to_ascii_lowercase();
        parser.skip_ws();
        let Some(open) = parser.peek() else { break };
        if open != b'{' && open != b'(' {
            // `@` in free text (an e-mail address in a comment): not an entry.
            continue;
        }
        let close = if open == b'{' { b'}' } else { b')' };
        parser.pos += 1;
        match kind.as_str() {
            "comment" | "preamble" => {
                parser
                    .skip_balanced(open, close)
                    .map_err(|()| unclosed(line))?;
            }
            "string" => {
                let fields = parser.fields(close).map_err(|e| at_line(&e, line))?;
                for (name, value) in fields {
                    parser.macros.insert(name, value);
                }
            }
            _ => {
                let key = parser.key();
                if key.is_empty() {
                    return Err(Error::Bib(format!(
                        "line {line}: @{kind} entry without a key"
                    )));
                }
                let raw = parser.fields(close).map_err(|e| at_line(&e, line))?;
                entries.push(build_entry(key, kind, raw));
            }
        }
    }
    Ok(Bibliography { entries })
}

fn unclosed(line: usize) -> Error {
    Error::Bib(format!("line {line}: unclosed entry (unbalanced braces)"))
}

fn at_line(err: &str, line: usize) -> Error {
    Error::Bib(format!("line {line}: {err}"))
}

/// Turn raw (undecoded) fields into an [`Entry`].
fn build_entry(key: String, kind: String, raw: Vec<(String, String)>) -> Entry {
    let mut fields = BTreeMap::new();
    let mut authors = Vec::new();
    let mut editors = Vec::new();
    for (name, value) in raw {
        match name.as_str() {
            "author" => authors = parse_names(&value),
            "editor" => editors = parse_names(&value),
            _ => {}
        }
        // Identifiers are verbatim: a `~` in a URL is a tilde, not a tie.
        let decoded = if VERBATIM_FIELDS.contains(&name.as_str()) {
            value.trim().to_string()
        } else {
            decode_latex(&value)
        };
        fields.insert(name, decoded);
    }
    Entry {
        key,
        kind,
        fields,
        authors,
        editors,
    }
}

/// Fields whose value is an identifier, kept as written.
const VERBATIM_FIELDS: &[&str] = &["url", "doi", "eprint", "file", "isbn", "issn", "pdf"];

/// The predefined month macros.
fn month_macros() -> HashMap<String, String> {
    [
        ("jan", "January"),
        ("feb", "February"),
        ("mar", "March"),
        ("apr", "April"),
        ("may", "May"),
        ("jun", "June"),
        ("jul", "July"),
        ("aug", "August"),
        ("sep", "September"),
        ("oct", "October"),
        ("nov", "November"),
        ("dec", "December"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

struct Parser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
    macros: HashMap<String, String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn line_of(&self, at: usize) -> usize {
        self.src[..at].iter().filter(|&&b| b == b'\n').count() + 1
    }

    /// The next `@` from the current position.
    fn find_at(&self) -> Option<usize> {
        self.src[self.pos.min(self.src.len())..]
            .iter()
            .position(|&b| b == b'@')
            .map(|p| p + self.pos)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// An identifier: letters, digits and the punctuation BibTeX allows in names.
    fn identifier(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b"_-:.+/'!?*".contains(&b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.text[start..self.pos].to_string()
    }

    /// An entry key: everything up to the first comma (or the end of the entry).
    fn key(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == b',' || b == b'}' || b == b')' || b.is_ascii_whitespace() {
                break;
            }
            self.pos += 1;
        }
        let key = self.text[start..self.pos].to_string();
        self.skip_ws();
        if self.peek() == Some(b',') {
            self.pos += 1;
        }
        key
    }

    /// Skip to the delimiter that closes the current block (we are just inside it).
    fn skip_balanced(&mut self, open: u8, close: u8) -> std::result::Result<(), ()> {
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
        }
        Err(())
    }

    /// `name = value, …` up to `close`. Names are lower-cased; values are raw (macros
    /// expanded, concatenated, outer delimiters removed) but not yet LaTeX-decoded.
    fn fields(&mut self, close: u8) -> std::result::Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err("unclosed entry (unbalanced braces)".into()),
                Some(b) if b == close => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b',') => {
                    self.pos += 1;
                    continue;
                }
                Some(_) => {}
            }
            let name = self.identifier().to_ascii_lowercase();
            if name.is_empty() {
                // Garbage: skip one byte so we always make progress.
                self.pos += 1;
                continue;
            }
            self.skip_ws();
            if self.peek() != Some(b'=') {
                // A field without a value: ignore it.
                continue;
            }
            self.pos += 1;
            let value = self.value()?;
            out.push((name, value));
        }
    }

    /// One value: parts joined by `#`.
    fn value(&mut self) -> std::result::Result<String, String> {
        let mut value = String::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'{') => {
                    self.pos += 1;
                    let start = self.pos;
                    let mut depth = 1usize;
                    loop {
                        match self.peek() {
                            None => return Err("unclosed brace in a field value".into()),
                            Some(b'{') => depth += 1,
                            Some(b'}') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Some(b'\\') => self.pos += 1,
                            Some(_) => {}
                        }
                        self.pos += 1;
                    }
                    value.push_str(&self.text[start..self.pos]);
                    self.pos += 1;
                }
                Some(b'"') => {
                    self.pos += 1;
                    let start = self.pos;
                    let mut depth = 0usize;
                    loop {
                        match self.peek() {
                            None => return Err("unclosed quote in a field value".into()),
                            Some(b'{') => depth += 1,
                            Some(b'}') => depth = depth.saturating_sub(1),
                            Some(b'"') if depth == 0 => break,
                            Some(b'\\') => self.pos += 1,
                            Some(_) => {}
                        }
                        self.pos += 1;
                    }
                    value.push_str(&self.text[start..self.pos]);
                    self.pos += 1;
                }
                Some(b) if b.is_ascii_digit() => {
                    let start = self.pos;
                    while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                        self.pos += 1;
                    }
                    value.push_str(&self.text[start..self.pos]);
                }
                Some(_) => {
                    let name = self.identifier();
                    if name.is_empty() {
                        return Err("a field has no value".into());
                    }
                    match self.macros.get(&name.to_ascii_lowercase()) {
                        Some(expansion) => value.push_str(expansion),
                        None => value.push_str(&name),
                    }
                }
                None => return Err("unclosed entry (unbalanced braces)".into()),
            }
            self.skip_ws();
            if self.peek() == Some(b'#') {
                self.pos += 1;
            } else {
                return Ok(value);
            }
        }
    }
}

/// Split a raw `author`/`editor` value into names (`A and B and others`).
///
/// `and` separates names only outside braces, so `{Barnes and Noble}` stays one
/// (corporate) name. `others` becomes a [`Name`] with `others` set.
pub(super) fn parse_names(raw: &str) -> Vec<Name> {
    split_top_level_and(raw)
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_name(s.trim()))
        .collect()
}

/// Split `raw` on the whole word `and` (case-insensitive) at brace depth 0.
fn split_top_level_and(raw: &str) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b if depth == 0 && b.is_ascii_whitespace() => {
                let word_end = i + 4;
                if bytes.len() > word_end
                    && raw.is_char_boundary(i + 1)
                    && raw.is_char_boundary(word_end)
                    && raw[i + 1..word_end].eq_ignore_ascii_case("and")
                    && bytes[word_end].is_ascii_whitespace()
                {
                    parts.push(&raw[start..i]);
                    start = word_end + 1;
                    i = word_end;
                }
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&raw[start.min(raw.len())..]);
    parts
}

/// Split on top-level commas.
fn split_top_level_commas(raw: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, b) in raw.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(&raw[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&raw[start..]);
    parts
}

/// Split on top-level whitespace (and `~`, BibTeX's tie).
fn split_top_level_spaces(raw: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (i, c) in raw.char_indices() {
        match c {
            '{' => {
                depth += 1;
                start.get_or_insert(i);
            }
            '}' => {
                depth = depth.saturating_sub(1);
            }
            c if depth == 0 && (c.is_whitespace() || c == '~') => {
                if let Some(s) = start.take() {
                    words.push(&raw[s..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s) = start {
        words.push(&raw[s..]);
    }
    words
}

/// Is this (raw) word a lower-case "von" particle? BibTeX's rule: its first letter is
/// lower case. A word that starts with an accent group (`{\'E}mile`) is judged by the
/// letter inside; a wholly brace-protected word (`{Barnes and Noble}`) never is one.
fn is_particle(word: &str) -> bool {
    if word.starts_with('{') && word.ends_with('}') && !word.starts_with("{\\") {
        return false;
    }
    decode_latex(word)
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(char::is_lowercase)
}

/// Parse one name in any of BibTeX's three forms.
fn parse_name(raw: &str) -> Name {
    if raw.eq_ignore_ascii_case("others") {
        return Name {
            others: true,
            ..Name::default()
        };
    }
    let parts = split_top_level_commas(raw);
    let join = |words: &[&str]| decode_latex(&words.join(" "));
    match parts.as_slice() {
        // "First von Last"
        [single] => {
            let words = split_top_level_spaces(single);
            if words.len() <= 1 {
                return Name {
                    family: join(&words),
                    ..Name::default()
                };
            }
            let last = words.len() - 1;
            // von part: from the first lower-case word to the last lower-case word that
            // is not the final word.
            match words[..last].iter().position(|w| is_particle(w)) {
                Some(fv) => {
                    let lv = (fv..last)
                        .rev()
                        .find(|&i| is_particle(words[i]))
                        .unwrap_or(fv);
                    Name {
                        given: join(&words[..fv]),
                        particle: join(&words[fv..=lv]),
                        family: join(&words[lv + 1..]),
                        ..Name::default()
                    }
                }
                None => Name {
                    given: join(&words[..last]),
                    family: join(&words[last..]),
                    ..Name::default()
                },
            }
        }
        // "von Last, First" / "von Last, Jr, First"
        [last_part, rest @ ..] => {
            let words = split_top_level_spaces(last_part);
            // The von part is the longest prefix of lower-case words, leaving at least
            // one word for the family name.
            let mut split = 0;
            while split + 1 < words.len() && is_particle(words[split]) {
                split += 1;
            }
            let (suffix, given) = match rest {
                [given] => (String::new(), decode_latex(given.trim())),
                [suffix, given, ..] => (decode_latex(suffix.trim()), decode_latex(given.trim())),
                [] => (String::new(), String::new()),
            };
            Name {
                given,
                particle: join(&words[..split]),
                family: join(&words[split..]),
                suffix,
                others: false,
            }
        }
        [] => Name::default(),
    }
}

/// Decode LaTeX markup in a BibTeX value into plain Unicode text.
///
/// Accents (`{\'e}`, `\'{e}`, `\"o`, `\c{c}`, `\v{s}`…), special letters (`\ss`, `\o`,
/// `\aa`, `\ae`, `\l`…), escaped characters (`\&`, `\%`, `\$`, `\_`, `\#`), dashes (`--`,
/// `---`), the tie `~`, and formatting commands (`\emph{x}`, `\textbf{x}`, `\url{x}`)
/// whose argument is kept; braces are dropped and runs of whitespace collapse to one
/// space.
///
/// # Examples
///
/// ```
/// use md2star_rs::bib::decode_latex;
/// assert_eq!(decode_latex(r#"Sch{\"u}tze"#), "Schütze");
/// assert_eq!(
///     decode_latex(r"Lemar\'{e}chal --- {\emph{Optimization}}"),
///     "Lemaréchal — Optimization"
/// );
/// ```
pub fn decode_latex(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' | '}' => i += 1,
            '~' => {
                out.push('\u{a0}');
                i += 1;
            }
            // TeX's quotes: `` and '' are double, ` and ' single (curly).
            '`' | '\'' => {
                let double = chars.get(i + 1) == Some(&c);
                out.push(match (c, double) {
                    ('`', true) => '“',
                    ('`', false) => '‘',
                    (_, true) => '”',
                    _ => '’',
                });
                i += if double { 2 } else { 1 };
            }
            '-' => {
                let mut n = 0;
                while chars.get(i + n) == Some(&'-') {
                    n += 1;
                }
                match n {
                    1 => out.push('-'),
                    2 => out.push('–'),
                    _ => out.push('—'),
                }
                i += n;
            }
            '\\' => {
                i += 1;
                let Some(&next) = chars.get(i) else { break };
                if next.is_ascii_alphabetic() {
                    let start = i;
                    while chars.get(i).is_some_and(char::is_ascii_alphabetic) {
                        i += 1;
                    }
                    let name: String = chars[start..i].iter().collect();
                    // A control word swallows the spaces after it.
                    let after_word = i;
                    while chars.get(i) == Some(&' ') {
                        i += 1;
                    }
                    if let Some(letter) = special_letter(&name) {
                        out.push_str(letter);
                    } else if let Some(mark) = accent_mark(&name) {
                        let (base, consumed) = accent_argument(&chars, i);
                        i += consumed;
                        push_accented(&mut out, &base, mark);
                    } else if chars.get(i) != Some(&'{') && after_word != i {
                        // Formatting or unknown command: the command goes, its argument
                        // stays (the main loop drops the braces). One with no argument
                        // keeps the space it swallowed.
                        out.push(' ');
                    }
                } else {
                    i += 1;
                    if let Some(mark) = accent_symbol(next) {
                        let (base, consumed) = accent_argument(&chars, i);
                        i += consumed;
                        push_accented(&mut out, &base, mark);
                    } else {
                        match next {
                            '\\' | ' ' => out.push(' '),
                            other => out.push(other),
                        }
                    }
                }
            }
            c if c.is_whitespace() => {
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
                while chars.get(i).is_some_and(|c| c.is_whitespace()) {
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    let collapsed: String = {
        let mut s = String::with_capacity(out.len());
        for c in out.chars() {
            if c == ' ' && s.ends_with(' ') {
                continue;
            }
            s.push(c);
        }
        s
    };
    collapsed.trim().to_string()
}

/// `\ss`, `\o`, `\aa` and friends.
fn special_letter(name: &str) -> Option<&'static str> {
    Some(match name {
        "ss" => "ß",
        "o" => "ø",
        "O" => "Ø",
        "aa" => "å",
        "AA" => "Å",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "l" => "ł",
        "L" => "Ł",
        "i" => "ı",
        "j" => "ȷ",
        "dh" => "ð",
        "DH" => "Ð",
        "th" => "þ",
        "TH" => "Þ",
        "ng" => "ŋ",
        "NG" => "Ŋ",
        "textendash" => "–",
        "textemdash" => "—",
        "textquoteright" => "’",
        "textquoteleft" => "‘",
        "textregistered" => "®",
        "texttrademark" => "™",
        "copyright" => "©",
        "S" => "§",
        "P" => "¶",
        "dag" => "†",
        "ldots" | "dots" | "textellipsis" => "…",
        "LaTeX" => "LaTeX",
        "TeX" => "TeX",
        _ => return None,
    })
}

/// Accent commands spelled with letters (`\c{c}`, `\v{s}`, `\H{o}`…), as combining marks.
fn accent_mark(name: &str) -> Option<char> {
    Some(match name {
        "c" => '\u{327}',
        "v" => '\u{30c}',
        "u" => '\u{306}',
        "H" => '\u{30b}',
        "k" => '\u{328}',
        "r" => '\u{30a}',
        "d" => '\u{323}',
        "b" => '\u{331}',
        "t" => '\u{361}',
        _ => return None,
    })
}

/// Accent commands spelled with a symbol (`\'`, `` \` ``, `\^`, `\"`, `\~`, `\=`, `\.`).
fn accent_symbol(symbol: char) -> Option<char> {
    Some(match symbol {
        '\'' => '\u{301}',
        '`' => '\u{300}',
        '^' => '\u{302}',
        '"' => '\u{308}',
        '~' => '\u{303}',
        '=' => '\u{304}',
        '.' => '\u{307}',
        _ => return None,
    })
}

/// The argument of an accent: `{e}`, `{\i}`, `\i`, or the next character. Returns the
/// decoded base and the number of chars consumed.
fn accent_argument(chars: &[char], at: usize) -> (String, usize) {
    match chars.get(at) {
        Some('{') => {
            let mut depth = 1;
            let mut j = at + 1;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let end = if depth == 0 { j - 1 } else { j };
            let inner: String = chars[at + 1..end].iter().collect();
            (decode_latex(&inner), j - at)
        }
        Some('\\') => {
            // `\'\i`: a control word as the base.
            let mut j = at + 1;
            while chars.get(j).is_some_and(char::is_ascii_alphabetic) {
                j += 1;
            }
            let name: String = chars[at + 1..j].iter().collect();
            (special_letter(&name).unwrap_or("").to_string(), j - at)
        }
        Some(c) => (c.to_string(), 1),
        None => (String::new(), 0),
    }
}

/// Push `base` with `mark` applied to its first letter, precomposed when we know the
/// composition (the common Latin letters), combining otherwise.
fn push_accented(out: &mut String, base: &str, mark: char) {
    let mut chars = base.chars();
    let Some(first) = chars.next() else {
        return;
    };
    let first = if first == 'ı' { 'i' } else { first };
    match compose(first, mark) {
        Some(c) => out.push(c),
        None => {
            out.push(first);
            out.push(mark);
        }
    }
    out.extend(chars);
}

/// Precompose the accented Latin letters BibTeX files actually contain.
fn compose(base: char, mark: char) -> Option<char> {
    const TABLE: &[(char, char, char)] = &[
        ('a', '\u{301}', 'á'),
        ('e', '\u{301}', 'é'),
        ('i', '\u{301}', 'í'),
        ('o', '\u{301}', 'ó'),
        ('u', '\u{301}', 'ú'),
        ('y', '\u{301}', 'ý'),
        ('c', '\u{301}', 'ć'),
        ('n', '\u{301}', 'ń'),
        ('s', '\u{301}', 'ś'),
        ('z', '\u{301}', 'ź'),
        ('A', '\u{301}', 'Á'),
        ('E', '\u{301}', 'É'),
        ('I', '\u{301}', 'Í'),
        ('O', '\u{301}', 'Ó'),
        ('U', '\u{301}', 'Ú'),
        ('Y', '\u{301}', 'Ý'),
        ('C', '\u{301}', 'Ć'),
        ('N', '\u{301}', 'Ń'),
        ('S', '\u{301}', 'Ś'),
        ('Z', '\u{301}', 'Ź'),
        ('a', '\u{300}', 'à'),
        ('e', '\u{300}', 'è'),
        ('i', '\u{300}', 'ì'),
        ('o', '\u{300}', 'ò'),
        ('u', '\u{300}', 'ù'),
        ('A', '\u{300}', 'À'),
        ('E', '\u{300}', 'È'),
        ('I', '\u{300}', 'Ì'),
        ('O', '\u{300}', 'Ò'),
        ('U', '\u{300}', 'Ù'),
        ('a', '\u{302}', 'â'),
        ('e', '\u{302}', 'ê'),
        ('i', '\u{302}', 'î'),
        ('o', '\u{302}', 'ô'),
        ('u', '\u{302}', 'û'),
        ('A', '\u{302}', 'Â'),
        ('E', '\u{302}', 'Ê'),
        ('I', '\u{302}', 'Î'),
        ('O', '\u{302}', 'Ô'),
        ('U', '\u{302}', 'Û'),
        ('a', '\u{308}', 'ä'),
        ('e', '\u{308}', 'ë'),
        ('i', '\u{308}', 'ï'),
        ('o', '\u{308}', 'ö'),
        ('u', '\u{308}', 'ü'),
        ('y', '\u{308}', 'ÿ'),
        ('A', '\u{308}', 'Ä'),
        ('E', '\u{308}', 'Ë'),
        ('I', '\u{308}', 'Ï'),
        ('O', '\u{308}', 'Ö'),
        ('U', '\u{308}', 'Ü'),
        ('a', '\u{303}', 'ã'),
        ('o', '\u{303}', 'õ'),
        ('n', '\u{303}', 'ñ'),
        ('A', '\u{303}', 'Ã'),
        ('O', '\u{303}', 'Õ'),
        ('N', '\u{303}', 'Ñ'),
        ('c', '\u{327}', 'ç'),
        ('C', '\u{327}', 'Ç'),
        ('s', '\u{327}', 'ş'),
        ('S', '\u{327}', 'Ş'),
        ('c', '\u{30c}', 'č'),
        ('s', '\u{30c}', 'š'),
        ('z', '\u{30c}', 'ž'),
        ('r', '\u{30c}', 'ř'),
        ('e', '\u{30c}', 'ě'),
        ('n', '\u{30c}', 'ň'),
        ('C', '\u{30c}', 'Č'),
        ('S', '\u{30c}', 'Š'),
        ('Z', '\u{30c}', 'Ž'),
        ('R', '\u{30c}', 'Ř'),
        ('E', '\u{30c}', 'Ě'),
        ('a', '\u{30a}', 'å'),
        ('A', '\u{30a}', 'Å'),
        ('u', '\u{30a}', 'ů'),
        ('o', '\u{30b}', 'ő'),
        ('u', '\u{30b}', 'ű'),
        ('O', '\u{30b}', 'Ő'),
        ('U', '\u{30b}', 'Ű'),
        ('a', '\u{328}', 'ą'),
        ('e', '\u{328}', 'ę'),
        ('a', '\u{304}', 'ā'),
        ('e', '\u{304}', 'ē'),
        ('i', '\u{304}', 'ī'),
        ('o', '\u{304}', 'ō'),
        ('u', '\u{304}', 'ū'),
        ('z', '\u{307}', 'ż'),
        ('Z', '\u{307}', 'Ż'),
        ('g', '\u{306}', 'ğ'),
        ('G', '\u{306}', 'Ğ'),
        ('a', '\u{306}', 'ă'),
    ];
    TABLE
        .iter()
        .find(|(b, m, _)| *b == base && *m == mark)
        .map(|(_, _, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_fields_and_delimiters() {
        let bib = parse(
            r#"
            % a comment line with an e-mail: someone@example.org
            @Book{k1,
              Title = {A {Protected} Title},
              author = "Doe, John",
              year = 2001,
              note = "Quoted " # {and braced}
            }
            @article(k2, title={Parens}, year={1999})
            "#,
        )
        .unwrap();
        assert_eq!(bib.entries.len(), 2);
        let k1 = bib.get("k1").unwrap();
        assert_eq!(k1.kind, "book");
        assert_eq!(k1.field("title"), Some("A Protected Title"));
        assert_eq!(k1.field("year"), Some("2001"));
        assert_eq!(k1.field("note"), Some("Quoted and braced"));
        assert_eq!(bib.get("k2").unwrap().field("title"), Some("Parens"));
        let urls = parse("@misc{u, url = {https://x.org/~me/a_b}, title = {~}}").unwrap();
        assert_eq!(urls.entries[0].field("url"), Some("https://x.org/~me/a_b"));
    }

    #[test]
    fn string_macros_and_months() {
        let bib = parse(
            "@string{ieee = \"IEEE\"}\n@STRING(pub = ieee # { Press})\n\
             @misc{m, publisher = pub, month = jan}",
        )
        .unwrap();
        let m = bib.get("m").unwrap();
        assert_eq!(m.field("publisher"), Some("IEEE Press"));
        assert_eq!(m.field("month"), Some("January"));
    }

    #[test]
    fn comment_and_preamble_are_skipped() {
        let bib = parse(
            "@comment{ @book{fake, title={no}} }\n@preamble{\"\\newcommand{\\x}{y}\"}\n\
             @book{real, title={yes}}",
        )
        .unwrap();
        assert_eq!(bib.entries.len(), 1);
        assert_eq!(bib.entries[0].key, "real");
    }

    #[test]
    fn unbalanced_braces_name_the_line() {
        let err = parse("\n\n@book{x, title = {open}").unwrap_err();
        match err {
            Error::Bib(msg) => assert!(msg.starts_with("line 3"), "{msg}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn latex_accents_decode() {
        assert_eq!(decode_latex(r"{\'e}t{\'e}"), "été");
        assert_eq!(decode_latex(r"\'{E}cole"), "École");
        assert_eq!(decode_latex(r#"G{\"o}del"#), "Gödel");
        assert_eq!(decode_latex(r"\c{c}a"), "ça");
        assert_eq!(decode_latex(r"\v{S}koda"), "Škoda");
        assert_eq!(decode_latex(r"Stra\ss e"), "Straße");
        assert_eq!(decode_latex(r"{\o}ster"), "øster");
        assert_eq!(decode_latex(r"\^{\i}le"), "île");
        assert_eq!(decode_latex(r"\~n"), "ñ");
        assert_eq!(decode_latex(r"R\&D 50\%"), "R&D 50%");
        assert_eq!(decode_latex("1--10"), "1–10");
        assert_eq!(decode_latex("a---b"), "a—b");
        assert_eq!(decode_latex("A.~B."), "A.\u{a0}B.");
        assert_eq!(
            decode_latex(r"\emph{Deep}  \textbf{Learning}"),
            "Deep Learning"
        );
        assert_eq!(decode_latex("multi\n   line"), "multi line");
        assert_eq!(
            decode_latex("l'intelligence ``quoted''"),
            "l’intelligence “quoted”"
        );
    }

    #[test]
    fn names_in_every_form() {
        let names = parse_names(r"Pearl, Judea and Charles F. Van Loan and Marcos Lopez de Prado");
        assert_eq!(names[0].family, "Pearl");
        assert_eq!(names[0].given, "Judea");
        // BibTeX's rule: a capitalised "Van" belongs to the first names.
        assert_eq!(names[1].family, "Loan");
        assert_eq!(names[1].given, "Charles F. Van");
        assert_eq!(names[2].family, "Prado");
        assert_eq!(names[2].particle, "de");
        assert_eq!(names[2].given, "Marcos Lopez");

        let names = parse_names(r"van der Waals, Johannes and Ford, Jr., Henry");
        assert_eq!(names[0].particle, "van der");
        assert_eq!(names[0].family, "Waals");
        assert_eq!(names[1].family, "Ford");
        assert_eq!(names[1].suffix, "Jr.");
        assert_eq!(names[1].given, "Henry");
    }

    #[test]
    fn protected_names_and_others() {
        let names = parse_names(r"{Barnes and Noble} and Smith, A. and others");
        assert_eq!(names.len(), 3);
        assert_eq!(names[0].family, "Barnes and Noble");
        assert!(names[0].given.is_empty());
        assert!(names[2].others);
        let names = parse_names(r#"Sch{\"u}tze, Hinrich and {\'E}mile Zola"#);
        assert_eq!(names[0].family, "Schütze");
        assert_eq!(names[1].given, "Émile");
        assert_eq!(names[1].family, "Zola");
    }

    #[test]
    fn the_fixture_bibliographies_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for (file, count) in [("refs.bib", 2), ("references.bib", 36)] {
            let src = std::fs::read_to_string(dir.join(file)).unwrap();
            let bib = parse(&src).unwrap();
            assert_eq!(bib.entries.len(), count, "{file}");
            assert!(bib.entries.iter().all(|e| !e.authors.is_empty()), "{file}");
        }
    }
}
