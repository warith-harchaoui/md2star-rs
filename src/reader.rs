//! Markdown reader: source text → [`Document`] (front matter + [`Block`]/[`Inline`] tree).
//!
//! Ownership: this module is the shared foundation, with [`crate::ast`]. Every backend
//! (DOCX, PPTX, PDF, twin) reads what it produces and none of them re-parses Markdown.
//!
//! # How a document is read
//!
//! 1. **Front matter.** A leading `---` … `---`/`...` block whose first line looks like a
//!    YAML key is cut off and parsed into [`Metadata`] (typed fields + the full free map).
//!    The "looks like a key" test matters: a deck may *open* with a `---` slide break, and
//!    eating its first slide as metadata would silently delete it (md2star's rule).
//! 2. **Pandoc syntax `pulldown-cmark` does not know** is marked up on the source text,
//!    outside code fences: each fenced-div fence (`::: {.columns}` / `:::`) becomes an
//!    HTML-comment sentinel on its own line — an HTML block CommonMark keeps intact and
//!    that may interrupt a paragraph — and the `^` of every inline note `^[…]` is swapped
//!    for a private-use character so the superscript parser cannot pair it.
//! 3. **`pulldown-cmark`** parses with tables, footnotes, strikethrough, task lists, math,
//!    superscript and subscript enabled. Its flat event stream is folded into a tree with a
//!    cursor (every `Start` has exactly one `End`, so a function that "consumes until its
//!    End" always leaves the cursor on the next sibling); a div sentinel opens a nested
//!    block list that the matching close sentinel ends.
//! 4. **A finishing pass** does what needs the whole tree: adjacent text is merged, then
//!    attributes are lifted (`{#id .class k="v"}` trailing a heading, an image or a link),
//!    inline notes and citations (`[@key, p. 3; @other]`, `@key`) are recognised in the
//!    text, math written inside a code span is unwrapped (md2star's rule), headings get
//!    Pandoc's automatic identifiers, an image alone in its paragraph becomes a
//!    [`Block::Figure`], a `Table: …` paragraph next to a table becomes its caption, and a
//!    `mermaid` code block becomes a [`Block::Mermaid`].
//!
//! The reader never fails: malformed input degrades to text, and anything worth knowing
//! (an unclosed div, front matter that is not YAML) is reported in
//! [`Document::warnings`].

use std::collections::{BTreeMap, HashSet};

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use yaml_rust2::{Yaml, YamlLoader};

use crate::ast::{
    plain_text, Align, Attr, Block, Citation, CitationMode, Document, Image, Inline, ListItem,
    MetaValue, Metadata, Warning,
};

/// Stands in for the `^` of an inline note `^[…]` while `pulldown-cmark` parses, so its
/// superscript rule (`^x^`) cannot pair two notes into one superscript. A private-use
/// code point: it never occurs in real text, and every occurrence the finishing pass
/// does not consume is turned back into `^`.
const NOTE_MARK: char = '\u{E000}';
/// Opening div sentinel prefix; the div's index into the attribute table follows.
const DIV_OPEN: &str = "<!--md2star-rs:div-open:";
/// Closing div sentinel.
const DIV_CLOSE: &str = "<!--md2star-rs:div-close-->";

/// Parse a Markdown string into its body blocks (front matter dropped).
///
/// Kept for the callers that only want the tree; [`read`] also returns the metadata
/// and the reader's warnings.
///
/// # Examples
///
/// ```
/// let doc = md2star_rs::reader::parse("# Title\n\nHello **world**.");
/// assert_eq!(doc.len(), 2); // the heading and the paragraph
/// ```
pub fn parse(markdown: &str) -> Vec<Block> {
    read(markdown).blocks
}

/// Read a Markdown document: front matter, body and warnings.
///
/// # Examples
///
/// ```
/// use md2star_rs::ast::{Block, Inline};
///
/// let doc = md2star_rs::reader::read("---\ntitle: Q3\nlang: fr-FR\n---\n\n# Growth {.bignumber}\n");
/// assert_eq!(doc.meta.title.as_deref(), Some("Q3"));
/// assert_eq!(doc.meta.lang.as_deref(), Some("fr-FR"));
/// match &doc.blocks[0] {
///     Block::Heading { attr, content, .. } => {
///         assert!(attr.has_class("bignumber"));
///         assert_eq!(content, &vec![Inline::Text("Growth".into())]);
///     }
///     other => panic!("expected a heading, got {other:?}"),
/// }
/// ```
pub fn read(markdown: &str) -> Document {
    let mut warnings = Vec::new();
    let text = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let (meta, body) = match split_front_matter(text) {
        Some((yaml, rest)) => (parse_metadata(yaml, &mut warnings), rest),
        None => (Metadata::default(), text),
    };

    let (prepared, divs) = prepare(body, &mut warnings);
    let events: Vec<Event> = Parser::new_ext(&prepared, parser_options()).collect();
    let mut cursor = Cursor {
        events,
        pos: 0,
        divs,
        pending_task: None,
    };
    let raw = blocks(&mut cursor, false);

    let mut ids = HashSet::new();
    let blocks = finish_blocks(raw, &mut ids);
    Document {
        meta,
        blocks,
        warnings,
    }
}

/// The `pulldown-cmark` extensions md2star's Markdown relies on.
fn parser_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_SUPERSCRIPT);
    options.insert(Options::ENABLE_SUBSCRIPT);
    options
}

// ───────────────────────────── front matter ─────────────────────────────

/// Split a leading YAML block off `text`: `Some((yaml, rest))`, or `None` when the
/// document has no front matter.
///
/// The block opens with `---` on the first line and closes with `---` or `...` (Pandoc's
/// rule). Its first non-blank line must look like `key:` — otherwise the `---` is a slide
/// break and the "block" is the first slide.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let yaml = &text[start..offset];
            let first_line = yaml.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            if !looks_like_yaml_key(first_line.trim()) {
                return None;
            }
            return Some((yaml, &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Does `line` start like a YAML mapping key (`title:`, `md2star_skip:`)?
fn looks_like_yaml_key(line: &str) -> bool {
    let mut chars = line.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let rest: String = chars
        .skip_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    rest.trim_start().starts_with(':')
}

/// Parse the YAML block into [`Metadata`]; a block that is not a YAML mapping yields
/// empty metadata and a warning.
fn parse_metadata(yaml: &str, warnings: &mut Vec<Warning>) -> Metadata {
    let docs = match YamlLoader::load_from_str(yaml) {
        Ok(docs) => docs,
        Err(e) => {
            warnings.push(Warning::new(
                "front-matter",
                format!("front matter is not valid YAML and was ignored: {e}"),
            ));
            return Metadata::default();
        }
    };
    let raw = match docs.into_iter().next().map(meta_value) {
        Some(MetaValue::Map(map)) => map,
        Some(MetaValue::Null) | None => BTreeMap::new(),
        Some(_) => {
            warnings.push(Warning::new(
                "front-matter",
                "front matter is not a YAML mapping and was ignored",
            ));
            BTreeMap::new()
        }
    };
    lift_metadata(raw)
}

/// Convert a `yaml-rust2` node into our owned [`MetaValue`]. Numbers keep their source
/// spelling (a `date: 2026-06-21` or a `version: 1.10` must not be reformatted).
fn meta_value(yaml: Yaml) -> MetaValue {
    match yaml {
        Yaml::String(s) | Yaml::Real(s) => MetaValue::String(s),
        Yaml::Integer(i) => MetaValue::String(i.to_string()),
        Yaml::Boolean(b) => MetaValue::Bool(b),
        Yaml::Array(items) => MetaValue::List(items.into_iter().map(meta_value).collect()),
        Yaml::Hash(hash) => MetaValue::Map(
            hash.into_iter()
                .map(|(k, v)| (yaml_key(k), meta_value(v)))
                .collect(),
        ),
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => MetaValue::Null,
    }
}

/// A mapping key as a string, whatever YAML scalar it was written as.
fn yaml_key(key: Yaml) -> String {
    match meta_value(key) {
        MetaValue::String(s) => s,
        MetaValue::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Lift the keys md2star gives a meaning to into typed fields; keep the whole map.
fn lift_metadata(raw: BTreeMap<String, MetaValue>) -> Metadata {
    let scalar = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| raw.get(*k).and_then(|v| v.as_str()))
    };
    let list = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .find_map(|k| raw.get(*k))
            .map(strings_of)
            .unwrap_or_default()
    };
    let authors = raw
        .get("author")
        .or_else(|| raw.get("authors"))
        .map(|v| match v {
            MetaValue::List(items) => items.iter().filter_map(person_name).collect(),
            other => person_name(other).into_iter().collect(),
        })
        .unwrap_or_default();
    let toc = ["toc", "table-of-contents"]
        .iter()
        .find_map(|k| raw.get(*k))
        .and_then(|v| match v {
            MetaValue::Bool(b) => Some(*b),
            MetaValue::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" => Some(true),
                "false" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        });
    Metadata {
        title: scalar(&["title"]),
        subtitle: scalar(&["subtitle"]),
        authors,
        date: scalar(&["date"]),
        date_format: scalar(&["date_format", "date-format"]),
        lang: scalar(&["lang"]),
        abstract_text: scalar(&["abstract"]),
        keywords: list(&["keywords"]),
        bibliography: list(&["bibliography"]),
        csl: scalar(&["csl"]),
        citation_style: scalar(&["citation-style", "citation_style", "style"]),
        reference_section_title: scalar(&[
            "reference-section-title",
            "bibliography-name",
            "bibliography_name",
        ]),
        toc,
        skip: list(&["md2star_skip"]),
        raw,
    }
}

/// A scalar or a list of scalars, as strings.
fn strings_of(value: &MetaValue) -> Vec<String> {
    match value {
        MetaValue::List(items) => items.iter().filter_map(MetaValue::as_str).collect(),
        other => other.as_str().into_iter().collect(),
    }
}

/// An author entry: a plain string or a `{name: …}` map.
fn person_name(value: &MetaValue) -> Option<String> {
    match value {
        MetaValue::Map(map) => map.get("name").and_then(MetaValue::as_str),
        other => other.as_str(),
    }
}

// ───────────────────────────── source preparation ─────────────────────────────

/// Mark up, outside code fences, the Pandoc syntax `pulldown-cmark` does not parse:
/// fenced divs become sentinel HTML comments and `^[` loses its `^` to [`NOTE_MARK`].
///
/// Returns the prepared text and the attribute table the div sentinels index into.
fn prepare(body: &str, warnings: &mut Vec<Warning>) -> (String, Vec<Attr>) {
    let mut out = String::with_capacity(body.len() + 64);
    let mut divs: Vec<Attr> = Vec::new();
    let mut open = 0usize;
    // The open code fence, if any: its character and run length.
    let mut fence: Option<(char, usize)> = None;

    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let eol = &line[content.len()..];
        let indent = content.len() - content.trim_start_matches(' ').len();
        let stripped = content.trim_start_matches(' ');

        if let Some((ch, len)) = fence {
            // Inside a fence everything is verbatim; only its closing run ends it.
            if indent <= 3
                && fence_run(stripped)
                    .is_some_and(|(c, n, rest)| c == ch && n >= len && rest.trim().is_empty())
            {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if indent <= 3 {
            if let Some((ch, len, _)) = fence_run(stripped) {
                fence = Some((ch, len));
                out.push_str(line);
                continue;
            }
            if let Some(attr) = div_open(stripped) {
                out.push_str(&format!("{DIV_OPEN}{}-->", divs.len()));
                out.push_str(eol_or_newline(eol));
                divs.push(attr);
                open += 1;
                continue;
            }
            if open > 0 && is_div_close(stripped) {
                out.push_str(DIV_CLOSE);
                out.push_str(eol_or_newline(eol));
                open -= 1;
                continue;
            }
        }
        out.push_str(&line.replace("^[", &format!("{NOTE_MARK}[")));
    }
    if open > 0 {
        warnings.push(Warning::new(
            "unclosed-div",
            format!("{open} fenced div(s) (`:::`) never closed; closed at the end of the document"),
        ));
        for _ in 0..open {
            out.push('\n');
            out.push_str(DIV_CLOSE);
            out.push('\n');
        }
    }
    (out, divs)
}

/// A line ending to put after a sentinel: the original one, or `\n` on the last line.
fn eol_or_newline(eol: &str) -> &str {
    if eol.is_empty() {
        "\n"
    } else {
        eol
    }
}

/// If `s` starts with a code-fence run (three or more `` ` `` or `~`), return the
/// character, the run length and what follows.
fn fence_run(s: &str) -> Option<(char, usize, &str)> {
    let ch = s.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = s.chars().take_while(|c| *c == ch).count();
    (n >= 3).then(|| (ch, n, &s[n..]))
}

/// Parse a div opening fence (`::: name`, `::: {#id .class}`, `:::: {.column}`).
fn div_open(s: &str) -> Option<Attr> {
    let colons = s.chars().take_while(|c| *c == ':').count();
    if colons < 3 {
        return None;
    }
    // A fence may be closed visually with trailing colons: `::: notes :::`.
    let rest = s[colons..].trim().trim_end_matches(':').trim();
    if rest.is_empty() {
        return None;
    }
    if let Some(inner) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        return parse_attr(inner);
    }
    let word_ok = !rest.contains(char::is_whitespace)
        && rest
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    word_ok.then(|| Attr {
        classes: vec![rest.to_string()],
        ..Attr::default()
    })
}

/// Is `s` a div closing fence (three or more colons and nothing else)?
fn is_div_close(s: &str) -> bool {
    let t = s.trim_end();
    t.len() >= 3 && t.chars().all(|c| c == ':')
}

// ───────────────────────────── attributes ─────────────────────────────

/// Parse the inside of a Pandoc attribute block: `#id .class key=value key="v w"`.
///
/// Returns `None` when any token is not an attribute, so that ordinary text in braces
/// (`{a, b}`) is never mistaken for one. `-` alone is Pandoc's shorthand for the
/// `unnumbered` class.
///
/// # Examples
///
/// ```
/// let attr = md2star_rs::reader::parse_attr(r#"#fig:a .wide layout="Big Number""#).unwrap();
/// assert_eq!(attr.id.as_deref(), Some("fig:a"));
/// assert!(attr.has_class("wide"));
/// assert_eq!(attr.get("layout"), Some("Big Number"));
/// assert!(md2star_rs::reader::parse_attr("a, b").is_none());
/// ```
pub fn parse_attr(s: &str) -> Option<Attr> {
    let mut attr = Attr::default();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.');
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '#' | '.' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && !chars[j].is_whitespace() {
                    j += 1;
                }
                let word: String = chars[start..j].iter().collect();
                if word.is_empty() || !word.chars().all(is_name) {
                    return None;
                }
                if c == '#' {
                    attr.id = Some(word);
                } else {
                    attr.classes.push(word);
                }
                i = j;
            }
            '-' if chars.get(i + 1).is_none_or(|n| n.is_whitespace()) => {
                attr.classes.push("unnumbered".to_string());
                i += 1;
            }
            _ => {
                let start = i;
                while i < chars.len() && is_name(chars[i]) && chars[i] != '.' {
                    i += 1;
                }
                let key: String = chars[start..i].iter().collect();
                if key.is_empty() || chars.get(i) != Some(&'=') {
                    return None;
                }
                i += 1; // the '='
                let value = match chars.get(i) {
                    Some(&q) if q == '"' || q == '\'' => {
                        let vstart = i + 1;
                        let mut j = vstart;
                        while j < chars.len() && chars[j] != q {
                            j += 1;
                        }
                        if j >= chars.len() {
                            return None; // unterminated quote
                        }
                        i = j + 1;
                        chars[vstart..j].iter().collect()
                    }
                    _ => {
                        let vstart = i;
                        while i < chars.len() && !chars[i].is_whitespace() {
                            i += 1;
                        }
                        chars[vstart..i].iter().collect()
                    }
                };
                attr.kv.push((key, value));
            }
        }
    }
    Some(attr)
}

/// Split a trailing `{…}` attribute block off `text`: the text before it (right-trimmed)
/// and the attributes, or `None` when `text` does not end with a valid one.
fn split_trailing_attr(text: &str) -> Option<(String, Attr)> {
    let trimmed = text.trim_end();
    let body = trimmed.strip_suffix('}')?;
    let open = body.rfind('{')?;
    let attr = parse_attr(&body[open + 1..])?;
    Some((body[..open].trim_end().to_string(), attr))
}

/// Split a code fence's info string into its language and attributes.
///
/// ```` ```python ````, ```` ```python {.numberLines} ````, ```` ```{.python #c} ```` and
/// MyST's ```` ```{mermaid} ```` are all understood.
fn parse_info(info: &str) -> (Option<String>, Attr) {
    let info = info.trim();
    if info.is_empty() {
        return (None, Attr::default());
    }
    if let Some(inner) = info.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        let inner = inner.trim();
        if let Some(attr) = parse_attr(inner) {
            return (attr.classes.first().cloned(), attr);
        }
        if !inner.is_empty() && !inner.contains(char::is_whitespace) {
            return (Some(inner.to_string()), Attr::default());
        }
        return (None, Attr::default());
    }
    let (word, rest) = match info.find(char::is_whitespace) {
        Some(at) => (&info[..at], info[at..].trim()),
        None => (info, ""),
    };
    let attr = rest
        .strip_prefix('{')
        .and_then(|r| r.strip_suffix('}'))
        .and_then(parse_attr)
        .unwrap_or_default();
    (Some(word.trim_start_matches('.').to_string()), attr)
}

// ───────────────────────────── event stream → tree ─────────────────────────────

/// A cursor over the collected event stream, plus the state the fold needs.
struct Cursor<'a> {
    events: Vec<Event<'a>>,
    pos: usize,
    /// Attributes of the fenced divs, indexed by their sentinel number.
    divs: Vec<Attr>,
    /// The task-list state of the item being read (set by `TaskListMarker`).
    pending_task: Option<bool>,
}

/// What a div sentinel at the cursor says.
enum Sentinel {
    Open(usize),
    Close,
}

impl<'a> Cursor<'a> {
    /// Return the next event and advance, or `None` at end of stream.
    fn next(&mut self) -> Option<Event<'a>> {
        let ev = self.events.get(self.pos).cloned();
        if ev.is_some() {
            self.pos += 1;
        }
        ev
    }

    /// Look at the next event without consuming it.
    fn peek(&self) -> Option<&Event<'a>> {
        self.events.get(self.pos)
    }

    /// Is the next HTML block one of our div sentinels?
    fn sentinel(&self) -> Option<Sentinel> {
        let (Some(Event::Start(Tag::HtmlBlock)), Some(Event::Html(html))) =
            (self.events.get(self.pos), self.events.get(self.pos + 1))
        else {
            return None;
        };
        let html = html.trim();
        if html == DIV_CLOSE {
            return Some(Sentinel::Close);
        }
        html.strip_prefix(DIV_OPEN)
            .and_then(|r| r.strip_suffix("-->"))
            .and_then(|n| n.parse().ok())
            .map(Sentinel::Open)
    }

    /// Consume a whole HTML block (through its `End`), returning its text.
    fn html_block(&mut self) -> String {
        let mut html = String::new();
        loop {
            match self.next() {
                Some(Event::Html(t)) | Some(Event::Text(t)) => html.push_str(&t),
                Some(Event::End(TagEnd::HtmlBlock)) | None => break,
                _ => {}
            }
        }
        html
    }
}

/// Consume block-level content until the stream ends, the enclosing container closes
/// (an `End`, left for the opener to eat), or — when `in_div` — a div close sentinel
/// (also left for the opener).
fn blocks(cursor: &mut Cursor, in_div: bool) -> Vec<Block> {
    let mut out = Vec::new();
    loop {
        match cursor.sentinel() {
            Some(Sentinel::Close) if in_div => break,
            Some(Sentinel::Close) => {
                cursor.next(); // Start(HtmlBlock)
                cursor.html_block();
                continue;
            }
            Some(Sentinel::Open(n)) => {
                cursor.next();
                cursor.html_block();
                let content = blocks(cursor, true);
                if matches!(cursor.sentinel(), Some(Sentinel::Close)) {
                    cursor.next();
                    cursor.html_block();
                }
                let attr = cursor.divs.get(n).cloned().unwrap_or_default();
                out.push(if attr.has_class("notes") {
                    Block::Notes(content)
                } else {
                    Block::Div { attr, content }
                });
                continue;
            }
            None => {}
        }

        let is_block_container = match cursor.peek() {
            None => break,
            Some(Event::End(_)) => break,
            Some(Event::Rule) => {
                cursor.next();
                out.push(Block::ThematicBreak);
                continue;
            }
            Some(Event::Start(tag)) => !is_inline_start(tag),
            Some(_) => false,
        };

        if is_block_container {
            if let Some(Event::Start(tag)) = cursor.next() {
                if let Some(block) = container(cursor, tag) {
                    out.push(block);
                }
            }
        } else {
            // Bare inline content: CommonMark's tight list items and block quotes.
            let content = gather_block_inlines(cursor);
            if !content.is_empty() {
                out.push(Block::Paragraph(content));
            }
        }
    }
    out
}

/// Is this an *inline* container tag (as opposed to a block-level one)?
fn is_inline_start(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

/// Collect bare inline content up to (not consuming) the next block boundary.
fn gather_block_inlines(cursor: &mut Cursor) -> Vec<Inline> {
    let mut out = Vec::new();
    loop {
        let stop = match cursor.peek() {
            None => true,
            Some(Event::End(_)) => true,
            Some(Event::Start(tag)) => !is_inline_start(tag),
            Some(Event::Rule) => true,
            Some(_) => false,
        };
        if stop {
            break;
        }
        let event = cursor.next().expect("peeked inline event");
        push_inline(event, cursor, &mut out);
    }
    out
}

/// Build one block from its just-consumed opening `tag`, eating the matching `End`.
fn container(cursor: &mut Cursor, tag: Tag) -> Option<Block> {
    match tag {
        Tag::Heading { level, .. } => Some(Block::Heading {
            level: heading_level(level),
            content: inlines(cursor),
            attr: Attr::default(),
        }),
        Tag::Paragraph => Some(Block::Paragraph(inlines(cursor))),

        Tag::CodeBlock(kind) => {
            let (lang, attr) = match kind {
                CodeBlockKind::Fenced(info) => parse_info(&info),
                CodeBlockKind::Indented => (None, Attr::default()),
            };
            let mut code = String::new();
            loop {
                match cursor.next() {
                    Some(Event::Text(t)) => code.push_str(&t),
                    Some(Event::End(_)) | None => break,
                    _ => {}
                }
            }
            let mermaid = lang.as_deref() == Some("mermaid") || attr.has_class("mermaid");
            Some(if mermaid {
                Block::Mermaid { code, attr }
            } else {
                Block::CodeBlock { lang, code, attr }
            })
        }

        Tag::BlockQuote(_) => {
            let inner = blocks(cursor, false);
            cursor.next(); // End(BlockQuote)
            Some(Block::BlockQuote(inner))
        }

        Tag::List(first) => {
            let ordered = first.is_some();
            let start = first.unwrap_or(1);
            let mut items = Vec::new();
            loop {
                match cursor.next() {
                    Some(Event::Start(Tag::Item)) => {
                        // A nested list resets and reads its own marker; keep ours aside.
                        let outer = cursor.pending_task.take();
                        let content = blocks(cursor, false);
                        let task = cursor.pending_task.take();
                        cursor.pending_task = outer;
                        items.push(ListItem { task, content });
                    }
                    Some(Event::End(TagEnd::List(_))) | None => break,
                    _ => {}
                }
            }
            Some(Block::List {
                ordered,
                start,
                items,
            })
        }

        Tag::Table(alignments) => {
            let aligns = alignments
                .iter()
                .map(|a| match a {
                    Alignment::None => Align::Default,
                    Alignment::Left => Align::Left,
                    Alignment::Center => Align::Center,
                    Alignment::Right => Align::Right,
                })
                .collect();
            let mut headers = Vec::new();
            let mut rows = Vec::new();
            loop {
                match cursor.next() {
                    Some(Event::Start(Tag::TableHead)) => headers = table_row(cursor),
                    Some(Event::Start(Tag::TableRow)) => rows.push(table_row(cursor)),
                    Some(Event::End(TagEnd::Table)) | None => break,
                    _ => {}
                }
            }
            Some(Block::Table {
                headers,
                rows,
                aligns,
                caption: Vec::new(),
                attr: Attr::default(),
            })
        }

        Tag::FootnoteDefinition(label) => {
            let content = blocks(cursor, false);
            cursor.next(); // End(FootnoteDefinition)
            Some(Block::FootnoteDef {
                label: label.to_string(),
                content,
            })
        }

        Tag::HtmlBlock => {
            let html = cursor.html_block();
            Some(Block::RawHtml(html))
        }

        // Anything else block-ish: parse its children so the cursor stays balanced and
        // keep them as a paragraph rather than losing them.
        _ => {
            let content = inlines(cursor);
            (!content.is_empty()).then_some(Block::Paragraph(content))
        }
    }
}

/// Consume one row's cells until the row's `End`.
fn table_row(cursor: &mut Cursor) -> Vec<Vec<Inline>> {
    let mut cells = Vec::new();
    loop {
        match cursor.next() {
            Some(Event::Start(Tag::TableCell)) => cells.push(inlines(cursor)),
            Some(Event::End(_)) | None => break,
            _ => {}
        }
    }
    cells
}

/// Consume inline content up to and *including* the closing `End` of the container.
fn inlines(cursor: &mut Cursor) -> Vec<Inline> {
    let mut out = Vec::new();
    loop {
        match cursor.next() {
            None | Some(Event::End(_)) => break,
            Some(event) => push_inline(event, cursor, &mut out),
        }
    }
    out
}

/// Translate one already-consumed inline `event` into [`Inline`]s.
fn push_inline(event: Event, cursor: &mut Cursor, out: &mut Vec<Inline>) {
    match event {
        Event::Text(t) => out.push(Inline::Text(t.to_string())),
        Event::Code(t) => out.push(Inline::Code(t.to_string())),
        Event::InlineMath(t) => out.push(Inline::Math {
            display: false,
            tex: t.to_string(),
        }),
        Event::DisplayMath(t) => out.push(Inline::Math {
            display: true,
            tex: t.to_string(),
        }),
        Event::Html(t) | Event::InlineHtml(t) => out.push(Inline::RawHtml(t.to_string())),
        Event::SoftBreak => out.push(Inline::SoftBreak),
        Event::HardBreak => out.push(Inline::HardBreak),
        Event::FootnoteReference(label) => out.push(Inline::FootnoteRef(label.to_string())),
        Event::TaskListMarker(checked) => cursor.pending_task = Some(checked),
        Event::Start(Tag::Emphasis) => out.push(Inline::Emph(inlines(cursor))),
        Event::Start(Tag::Strong) => out.push(Inline::Strong(inlines(cursor))),
        Event::Start(Tag::Strikethrough) => out.push(Inline::Strikeout(inlines(cursor))),
        Event::Start(Tag::Superscript) => out.push(Inline::Superscript(inlines(cursor))),
        Event::Start(Tag::Subscript) => out.push(Inline::Subscript(inlines(cursor))),
        Event::Start(Tag::Link {
            dest_url, title, ..
        }) => out.push(Inline::Link {
            text: inlines(cursor),
            url: dest_url.to_string(),
            title: (!title.is_empty()).then(|| title.to_string()),
            attr: Attr::default(),
        }),
        Event::Start(Tag::Image {
            dest_url, title, ..
        }) => out.push(Inline::Image(Image {
            alt: inlines(cursor),
            src: dest_url.to_string(),
            title: (!title.is_empty()).then(|| title.to_string()),
            attr: Attr::default(),
        })),
        Event::Start(_) => {
            let _ = inlines(cursor);
        }
        Event::Rule => {}
        _ => {}
    }
}

/// Map `pulldown-cmark`'s `HeadingLevel` to `1..=6`.
fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ───────────────────────────── finishing pass ─────────────────────────────

/// Finish a list of sibling blocks: normalise every block, then attach table captions.
fn finish_blocks(blocks: Vec<Block>, ids: &mut HashSet<String>) -> Vec<Block> {
    let finished: Vec<Block> = blocks.into_iter().map(|b| finish_block(b, ids)).collect();
    attach_table_captions(finished)
}

/// Finish one block (see the module docs, step 4).
fn finish_block(block: Block, ids: &mut HashSet<String>) -> Block {
    match block {
        Block::Paragraph(content) => {
            let content = normalize_inlines(content);
            match content.as_slice() {
                [Inline::Image(image)] if !image.alt.is_empty() => Block::Figure(image.clone()),
                _ => Block::Paragraph(content),
            }
        }
        Block::Heading {
            level,
            content,
            attr: _,
        } => {
            let mut content = normalize_inlines(content);
            let mut attr = Attr::default();
            if let Some(Inline::Text(last)) = content.last_mut() {
                if let Some((before, found)) = split_trailing_attr(last) {
                    *last = before;
                    attr = found;
                }
            }
            content.retain(|i| !matches!(i, Inline::Text(t) if t.is_empty()));
            let id = match attr.id.take() {
                Some(id) => id,
                None => unique_id(auto_identifier(&plain_text(&content)), ids),
            };
            ids.insert(id.clone());
            attr.id = Some(id);
            Block::Heading {
                level,
                content,
                attr,
            }
        }
        Block::CodeBlock { lang, code, attr } => Block::CodeBlock {
            lang,
            code: restore_marks(&code),
            attr,
        },
        Block::Mermaid { code, attr } => Block::Mermaid {
            code: restore_marks(&code),
            attr,
        },
        Block::BlockQuote(inner) => Block::BlockQuote(finish_blocks(inner, ids)),
        Block::List {
            ordered,
            start,
            items,
        } => Block::List {
            ordered,
            start,
            items: items
                .into_iter()
                .map(|item| ListItem {
                    task: item.task,
                    content: finish_blocks(item.content, ids),
                })
                .collect(),
        },
        Block::Table {
            headers,
            rows,
            aligns,
            caption,
            attr,
        } => Block::Table {
            headers: headers.into_iter().map(normalize_inlines).collect(),
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(normalize_inlines).collect())
                .collect(),
            aligns,
            caption,
            attr,
        },
        Block::Div { attr, content } => Block::Div {
            attr,
            content: finish_blocks(content, ids),
        },
        Block::Notes(content) => Block::Notes(finish_blocks(content, ids)),
        Block::FootnoteDef { label, content } => Block::FootnoteDef {
            label,
            content: finish_blocks(content, ids),
        },
        Block::RawHtml(html) => Block::RawHtml(restore_marks(&html)),
        other @ (Block::Figure(_) | Block::ThematicBreak) => other,
    }
}

/// Give a table without a caption the `Table: …` / `: …` paragraph right after it (or,
/// failing that, right before it), as Pandoc does. Also accepts the caption GFM swallowed
/// as the table's last row when no blank line separated them.
fn attach_table_captions(blocks: Vec<Block>) -> Vec<Block> {
    let mut blocks: Vec<Option<Block>> = blocks.into_iter().map(Some).collect();
    for i in 0..blocks.len() {
        let Some(Block::Table { caption, rows, .. }) = blocks[i].as_mut() else {
            continue;
        };
        if !caption.is_empty() {
            continue;
        }
        // The caption swallowed as a last row: one non-empty cell starting `Table:`/`:`.
        if let Some(last) = rows.last() {
            let rest_empty = last.iter().skip(1).all(|c| c.is_empty());
            if rest_empty {
                if let Some(found) = last.first().and_then(|c| caption_of(c)) {
                    rows.pop();
                    set_caption(blocks[i].as_mut(), found);
                    continue;
                }
            }
        }
        let after = blocks.get(i + 1).and_then(|b| match b {
            Some(Block::Paragraph(p)) => caption_of(p),
            _ => None,
        });
        if let Some(found) = after {
            blocks[i + 1] = None;
            set_caption(blocks[i].as_mut(), found);
            continue;
        }
        if i > 0 {
            let before = match &blocks[i - 1] {
                Some(Block::Paragraph(p)) => caption_of(p),
                _ => None,
            };
            if let Some(found) = before {
                blocks[i - 1] = None;
                set_caption(blocks[i].as_mut(), found);
            }
        }
    }
    blocks.into_iter().flatten().collect()
}

/// Store a caption (and its trailing attributes) on a table block.
fn set_caption(block: Option<&mut Block>, (inlines, found): (Vec<Inline>, Attr)) {
    if let Some(Block::Table { caption, attr, .. }) = block {
        *caption = inlines;
        *attr = found;
    }
}

/// If `inlines` read as a table caption (`Table: x` or `: x`), return the caption
/// without its prefix, and any trailing attributes.
fn caption_of(inlines: &[Inline]) -> Option<(Vec<Inline>, Attr)> {
    let Some(Inline::Text(first)) = inlines.first() else {
        return None;
    };
    let rest = first
        .strip_prefix("Table:")
        .or_else(|| first.strip_prefix(':').filter(|r| r.starts_with(' ')))?;
    let mut caption = inlines.to_vec();
    caption[0] = Inline::Text(rest.trim_start().to_string());
    let mut attr = Attr::default();
    if let Some(Inline::Text(last)) = caption.last_mut() {
        if let Some((before, found)) = split_trailing_attr(last) {
            *last = before;
            attr = found;
        }
    }
    caption.retain(|i| !matches!(i, Inline::Text(t) if t.is_empty()));
    Some((caption, attr))
}

/// Pandoc's automatic identifier for a heading's text.
///
/// Keep letters, digits, `_`, `-`, `.` and whitespace; lower-case; join the words with
/// `-` (so `A & B` gives `a-b`, as Pandoc does); drop everything before the first
/// letter; `section` when nothing is left.
fn auto_identifier(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.') || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let id = kept.split_whitespace().collect::<Vec<_>>().join("-");
    let start = id.find(|c: char| c.is_alphabetic()).unwrap_or(id.len());
    let id = &id[start..];
    if id.is_empty() {
        "section".to_string()
    } else {
        id.to_string()
    }
}

/// `base`, or `base-1`, `base-2`, … — the first one not used yet.
fn unique_id(base: String, ids: &HashSet<String>) -> String {
    if !ids.contains(&base) {
        return base;
    }
    (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !ids.contains(candidate))
        .expect("an unused suffix exists")
}

/// Undo the [`NOTE_MARK`] substitution in text that was not an inline note.
fn restore_marks(s: &str) -> String {
    s.replace(NOTE_MARK, "^")
}

/// Normalise a run of inlines (see the module docs, step 4).
fn normalize_inlines(inlines: Vec<Inline>) -> Vec<Inline> {
    // Children first, so every pass below sees finished nested content.
    let inlines: Vec<Inline> = inlines.into_iter().map(normalize_children).collect();
    let inlines = merge_text(inlines);
    let inlines = lift_link_attrs(inlines);
    let inlines = inline_notes(inlines);
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) => {
                for piece in split_scripts(&restore_marks(&t)) {
                    match piece {
                        Inline::Text(t) => out.extend(split_citations(&t)),
                        other => out.push(other),
                    }
                }
            }
            Inline::Code(c) => out.push(code_span_math(&c).unwrap_or(Inline::Code(c))),
            other => out.push(other),
        }
    }
    merge_text(out)
}

/// Normalise the inline children of one inline.
fn normalize_children(inline: Inline) -> Inline {
    match inline {
        Inline::Emph(c) => Inline::Emph(normalize_inlines(c)),
        Inline::Strong(c) => Inline::Strong(normalize_inlines(c)),
        Inline::Strikeout(c) => Inline::Strikeout(normalize_inlines(c)),
        Inline::Superscript(c) => Inline::Superscript(normalize_inlines(c)),
        Inline::Subscript(c) => Inline::Subscript(normalize_inlines(c)),
        Inline::Link {
            text,
            url,
            title,
            attr,
        } => Inline::Link {
            text: normalize_inlines(text),
            url,
            title,
            attr,
        },
        Inline::Image(mut image) => {
            image.alt = normalize_inlines(image.alt);
            Inline::Image(image)
        }
        Inline::Code(c) => Inline::Code(restore_marks(&c)),
        Inline::RawHtml(h) => Inline::RawHtml(restore_marks(&h)),
        other => other,
    }
}

/// Merge adjacent `Text` nodes and drop empty ones.
fn merge_text(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match (out.last_mut(), inline) {
            (_, Inline::Text(t)) if t.is_empty() => {}
            (Some(Inline::Text(prev)), Inline::Text(t)) => prev.push_str(&t),
            (_, other) => out.push(other),
        }
    }
    out
}

/// Move a `{…}` attribute block that directly follows an image or a link onto it.
fn lift_link_attrs(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        if let Inline::Text(text) = &inline {
            let takes_attr = matches!(out.last(), Some(Inline::Image(_) | Inline::Link { .. }));
            if takes_attr && text.starts_with('{') {
                if let Some(close) = text.find('}') {
                    if let Some(found) = parse_attr(&text[1..close]) {
                        match out.last_mut() {
                            Some(Inline::Image(image)) => image.attr = found,
                            Some(Inline::Link { attr, .. }) => *attr = found,
                            _ => {}
                        }
                        let rest = &text[close + 1..];
                        if !rest.is_empty() {
                            out.push(Inline::Text(rest.to_string()));
                        }
                        continue;
                    }
                }
            }
        }
        out.push(inline);
    }
    out
}

/// Turn every `^[…]` (its `^` swapped for [`NOTE_MARK`]) into an [`Inline::InlineNote`].
///
/// The closing bracket may sit in a later sibling text (`^[a *b* c]` spans three
/// inlines); brackets are counted so `^[see [1]]` closes at the right one. An unmatched
/// mark is left for [`restore_marks`].
fn inline_notes(inlines: Vec<Inline>) -> Vec<Inline> {
    let marker = format!("{NOTE_MARK}[");
    let mut queue: std::collections::VecDeque<Inline> = inlines.into();
    let mut out = Vec::new();
    while let Some(inline) = queue.pop_front() {
        let Inline::Text(text) = &inline else {
            out.push(inline);
            continue;
        };
        let Some(at) = text.find(&marker) else {
            out.push(inline);
            continue;
        };
        // Search for the matching `]`: first in the rest of this text, then onwards.
        let mut depth = 0usize;
        let mut inner: Vec<Inline> = Vec::new();
        let mut found: Option<(usize, String)> = None; // (siblings consumed, tail text)
        let head = text[..at].to_string();
        let mut segment = text[at + marker.len()..].to_string();
        let mut consumed = 0usize;
        loop {
            if let Some(close) = find_close(&segment, &mut depth) {
                inner.push(Inline::Text(segment[..close].to_string()));
                found = Some((consumed, segment[close + 1..].to_string()));
                break;
            }
            inner.push(Inline::Text(std::mem::take(&mut segment)));
            match queue.get(consumed) {
                None => break,
                Some(Inline::Text(t)) => {
                    segment = t.clone();
                    consumed += 1;
                }
                Some(other) => {
                    inner.push(other.clone());
                    consumed += 1;
                }
            }
        }
        match found {
            Some((consumed, tail)) => {
                queue.drain(..consumed);
                if !head.is_empty() {
                    out.push(Inline::Text(head));
                }
                out.push(Inline::InlineNote(normalize_inlines(inner)));
                if !tail.is_empty() {
                    queue.push_front(Inline::Text(tail));
                }
            }
            None => {
                // No closing bracket anywhere: keep the text, mark restored to `^`.
                let mut kept = head;
                kept.push('^');
                kept.push_str(&text[at + NOTE_MARK.len_utf8()..]);
                out.push(Inline::Text(kept));
            }
        }
    }
    merge_text(out)
}

/// Index of the `]` that closes the current bracket depth in `s`, updating `depth` as
/// nested `[`/`]` pairs go by.
fn find_close(s: &str, depth: &mut usize) -> Option<usize> {
    for (i, c) in s.char_indices() {
        match c {
            '[' => *depth += 1,
            ']' if *depth == 0 => return Some(i),
            ']' => *depth -= 1,
            _ => {}
        }
    }
    None
}

/// Pandoc's superscript `^x^` and subscript `~x~` inside a word (`2^10^`, `H~2~O`),
/// which `pulldown-cmark` only recognises between spaces: the content is non-empty and
/// has no whitespace, and `~~` (strikeout) is left alone.
fn split_scripts(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['^', '~']) {
        let mark = rest[at..].chars().next().expect("found");
        let after = &rest[at + 1..];
        let doubled = after.starts_with(mark) || rest[..at].ends_with(mark);
        let close = after.find(mark);
        match close {
            Some(close)
                if !doubled
                    && close > 0
                    && !after[..close].contains(char::is_whitespace)
                    && !after[close + 1..].starts_with(mark) =>
            {
                plain.push_str(&rest[..at]);
                flush_text(&mut plain, &mut out);
                let inner = vec![Inline::Text(after[..close].to_string())];
                out.push(if mark == '^' {
                    Inline::Superscript(inner)
                } else {
                    Inline::Subscript(inner)
                });
                rest = &after[close + 1..];
            }
            _ => {
                plain.push_str(&rest[..at + 1]);
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    flush_text(&mut plain, &mut out);
    out
}

// ───────────────────────────── citations ─────────────────────────────

/// Split citations out of a text run: bracketed groups (`[see @a, p. 3; -@b]`) and
/// in-text ones (`@a`, `@a [p. 3]`). Text that does not parse as a citation stays text.
fn split_citations(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().expect("in bounds");
        if c == '[' {
            if let Some(close) = text[i + 1..].find(']') {
                let inner = &text[i + 1..i + 1 + close];
                if !inner.contains('[') {
                    if let Some(cites) = bracket_citations(inner) {
                        flush_text(&mut plain, &mut out);
                        out.push(Inline::Cite(cites));
                        i += close + 2;
                        continue;
                    }
                }
            }
        }
        if c == '@' {
            let prev_ok = text[..i]
                .chars()
                .next_back()
                .is_none_or(|p| !(p.is_alphanumeric() || p == '_' || p == '.'));
            if prev_ok {
                if let Some((key, len)) = citation_key(&text[i + 1..]) {
                    let mut end = i + 1 + len;
                    let mut suffix = String::new();
                    // `@key [p. 3]`: a bracketed locator right after an in-text citation.
                    let after = &text[end..];
                    let gap = after.len() - after.trim_start_matches(' ').len();
                    if gap <= 1 && after[gap..].starts_with('[') {
                        if let Some(close) = after[gap + 1..].find(']') {
                            let loc = &after[gap + 1..gap + 1 + close];
                            if !loc.contains('@') && !loc.contains('[') {
                                suffix = loc.trim().to_string();
                                end += gap + close + 2;
                            }
                        }
                    }
                    flush_text(&mut plain, &mut out);
                    out.push(Inline::Cite(vec![Citation {
                        key,
                        prefix: String::new(),
                        suffix,
                        mode: CitationMode::AuthorInText,
                    }]));
                    i = end;
                    continue;
                }
            }
        }
        plain.push(c);
        i += c.len_utf8();
    }
    flush_text(&mut plain, &mut out);
    out
}

/// Push accumulated plain text as a `Text` node.
fn flush_text(plain: &mut String, out: &mut Vec<Inline>) {
    if !plain.is_empty() {
        out.push(Inline::Text(std::mem::take(plain)));
    }
}

/// Parse the inside of `[…]` as a citation group; `None` unless every `;`-separated
/// item carries a `@key`.
fn bracket_citations(inner: &str) -> Option<Vec<Citation>> {
    if !inner.contains('@') {
        return None;
    }
    let mut cites = Vec::new();
    for item in inner.split(';') {
        let at = item.find('@')?;
        let before = &item[..at];
        let (prefix, mode) = match before.strip_suffix('-') {
            Some(p) => (p, CitationMode::SuppressAuthor),
            None => (before, CitationMode::Normal),
        };
        // `@` must start a word: `a@b` is an address, not a citation.
        if prefix
            .chars()
            .next_back()
            .is_some_and(|p| p.is_alphanumeric())
        {
            return None;
        }
        let (key, len) = citation_key(&item[at + 1..])?;
        let rest = item[at + 1 + len..].trim();
        let suffix = rest.strip_prefix(',').unwrap_or(rest).trim();
        cites.push(Citation {
            key,
            prefix: prefix.trim().to_string(),
            suffix: suffix.to_string(),
            mode,
        });
    }
    (!cites.is_empty()).then_some(cites)
}

/// Read a citation key at the start of `s` (just after the `@`): `{any key}` or a run
/// starting with a letter, digit or `_` and continuing with those or internal
/// punctuation (`:.#$%&-+?<>~/`). Trailing punctuation is not part of the key. Returns
/// the key and how many bytes of `s` it spans.
fn citation_key(s: &str) -> Option<(String, usize)> {
    if let Some(rest) = s.strip_prefix('{') {
        let close = rest.find('}')?;
        let key = &rest[..close];
        return (!key.is_empty()).then(|| (key.to_string(), close + 2));
    }
    let first = s.chars().next()?;
    if !(first.is_alphanumeric() || first == '_') {
        return None;
    }
    let mut last_word_end = 0;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() || c == '_' {
            last_word_end = i + c.len_utf8();
        } else if !":.#$%&-+?<>~/".contains(c) {
            break;
        }
    }
    Some((s[..last_word_end].to_string(), last_word_end))
}

// ───────────────────────────── math in code spans ─────────────────────────────

/// md2star's rule: a code span holding LaTeX math is math, not code.
///
/// `` `$x^2$` `` becomes that formula; `` `quality_threshold $\in [0,1]$` `` becomes one
/// formula with the prose in `\text{…}` (underscores read as word separators, since a
/// bare `_` would mean a subscript). A span with no math stays code (`None`).
fn code_span_math(content: &str) -> Option<Inline> {
    let parts = split_math(content);
    let chunks: Vec<&(bool, bool, String)> = parts.iter().filter(|p| p.0).collect();
    if chunks.is_empty() {
        return None;
    }
    let display = chunks.iter().any(|c| c.1);
    let has_prose = parts.iter().any(|p| !p.0 && !p.2.trim().is_empty());
    if !has_prose && chunks.len() == 1 {
        return Some(Inline::Math {
            display,
            tex: chunks[0].2.trim().to_string(),
        });
    }
    let mut pieces: Vec<String> = Vec::new();
    for (is_math, _, text) in &parts {
        if *is_math {
            let t = text.trim();
            if !t.is_empty() {
                pieces.push(t.to_string());
            }
        } else {
            let prose = text.trim();
            if !prose.is_empty() {
                let words: Vec<&str> = prose.split('_').filter(|w| !w.is_empty()).collect();
                pieces.push(format!("\\text{{{}}}", words.join(" ")));
            }
        }
    }
    (!pieces.is_empty()).then(|| Inline::Math {
        display,
        tex: pieces.join(" "),
    })
}

/// Split `s` into `(is_math, is_display, text)` parts on `$$…$$`, `$…$`, `\(…\)` and
/// `\[…\]` (longest delimiters first; no newline or delimiter inside, non-empty).
fn split_math(s: &str) -> Vec<(bool, bool, String)> {
    let delims: [(&str, &str, bool); 4] = [
        ("$$", "$$", true),
        ("\\[", "\\]", true),
        ("$", "$", false),
        ("\\(", "\\)", false),
    ];
    let mut parts = Vec::new();
    let mut prose = String::new();
    let mut i = 0;
    'outer: while i < s.len() {
        for (open, close, display) in delims {
            if s[i..].starts_with(open) {
                let body_start = i + open.len();
                if let Some(rel) = s[body_start..].find(close) {
                    let body = &s[body_start..body_start + rel];
                    let bad = body.is_empty()
                        || body.contains('\n')
                        || (open == "$" && body.contains('$'));
                    if !bad {
                        if !prose.is_empty() {
                            parts.push((false, false, std::mem::take(&mut prose)));
                        }
                        parts.push((true, display, body.to_string()));
                        i = body_start + rel + close.len();
                        continue 'outer;
                    }
                }
            }
        }
        let c = s[i..].chars().next().expect("in bounds");
        prose.push(c);
        i += c.len_utf8();
    }
    if !prose.is_empty() {
        parts.push((false, false, prose));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(md: &str) -> Vec<Inline> {
        match parse(md).into_iter().next() {
            Some(Block::Paragraph(p)) => p,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    fn text(s: &str) -> Inline {
        Inline::Text(s.to_string())
    }

    // ── front matter ──

    #[test]
    fn front_matter_is_typed_and_kept_whole() {
        let doc = read(
            "---\ntitle: \"Q3 review\"\nsubtitle: Numbers\nauthor:\n  - Ada\n  - name: Grace\n\
             date: 2026-06-21\nlang: fr-FR\nbibliography: [a.bib, b.bib]\ncsl: apa.csl\n\
             citation-style: numeric\nreference-section-title: Sources\ntoc: true\n\
             keywords: x\nmd2star_skip: [language]\ncustom:\n  deep: [1, 2]\n---\n\nBody\n",
        );
        let m = &doc.meta;
        assert_eq!(m.title.as_deref(), Some("Q3 review"));
        assert_eq!(m.subtitle.as_deref(), Some("Numbers"));
        assert_eq!(m.authors, vec!["Ada", "Grace"]);
        assert_eq!(m.date.as_deref(), Some("2026-06-21"));
        assert_eq!(m.lang.as_deref(), Some("fr-FR"));
        assert_eq!(m.bibliography, vec!["a.bib", "b.bib"]);
        assert_eq!(m.csl.as_deref(), Some("apa.csl"));
        assert_eq!(m.citation_style.as_deref(), Some("numeric"));
        assert_eq!(m.reference_section_title.as_deref(), Some("Sources"));
        assert_eq!(m.toc, Some(true));
        assert_eq!(m.keywords, vec!["x"]);
        assert_eq!(m.skip, vec!["language"]);
        match m.raw.get("custom") {
            Some(MetaValue::Map(map)) => assert_eq!(
                map.get("deep"),
                Some(&MetaValue::List(vec![
                    MetaValue::String("1".into()),
                    MetaValue::String("2".into())
                ]))
            ),
            other => panic!("custom map lost: {other:?}"),
        }
        assert_eq!(doc.blocks, vec![Block::Paragraph(vec![text("Body")])]);
    }

    #[test]
    fn a_single_author_string_is_one_author() {
        let doc = read("---\nauthor: Warith Harchaoui\n---\n");
        assert_eq!(doc.meta.authors, vec!["Warith Harchaoui"]);
    }

    #[test]
    fn a_deck_opening_with_a_slide_break_keeps_its_first_slide() {
        let doc = read("---\n\n# First\n\n---\n\n# Second\n");
        assert_eq!(doc.meta, Metadata::default());
        let headings = doc
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Heading { .. }))
            .count();
        assert_eq!(headings, 2);
    }

    #[test]
    fn invalid_yaml_is_a_warning_not_a_failure() {
        let doc = read("---\ntitle: [unclosed\n---\n\nBody\n");
        assert!(doc.warnings.iter().any(|w| w.code == "front-matter"));
        assert_eq!(doc.blocks, vec![Block::Paragraph(vec![text("Body")])]);
    }

    #[test]
    fn dots_close_front_matter_too() {
        let doc = read("---\ntitle: T\n...\nBody\n");
        assert_eq!(doc.meta.title.as_deref(), Some("T"));
    }

    // ── headings and attributes ──

    #[test]
    fn heading_attributes_are_lifted() {
        let blocks = parse("## Growth {#g .bignumber layout=\"Big Number\"}\n");
        let Block::Heading {
            level,
            content,
            attr,
        } = &blocks[0]
        else {
            panic!("not a heading");
        };
        assert_eq!(*level, 2);
        assert_eq!(content, &vec![text("Growth")]);
        assert_eq!(attr.id.as_deref(), Some("g"));
        assert_eq!(attr.classes, vec!["bignumber"]);
        assert_eq!(attr.get("layout"), Some("Big Number"));
    }

    #[test]
    fn headings_get_pandoc_auto_identifiers_deduplicated() {
        let blocks = parse(
            "# Hello, World!\n\n# Hello, World!\n\n# 1. Intro\n\n# ???\n\n# Mermaid & Math\n",
        );
        let ids: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading { attr, .. } => attr.id.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                "hello-world",
                "hello-world-1",
                "intro",
                "section",
                "mermaid-math"
            ]
        );
    }

    #[test]
    fn braces_that_are_not_attributes_stay_text() {
        let blocks = parse("# Sets {a, b}\n");
        let Block::Heading { content, attr, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(content, &vec![text("Sets {a, b}")]);
        assert!(attr.classes.is_empty());
    }

    #[test]
    fn unnumbered_shorthand() {
        let blocks = parse("# Annexe {-}\n");
        let Block::Heading { attr, .. } = &blocks[0] else {
            panic!()
        };
        assert!(attr.has_class("unnumbered"));
    }

    // ── images, links, figures ──

    #[test]
    fn an_inline_image_keeps_source_alt_title_and_attributes() {
        let p = para("See ![a *logo*](img/logo.png \"The logo\"){width=50% .wide} here.");
        assert_eq!(p[0], text("See "));
        let Inline::Image(image) = &p[1] else {
            panic!("{p:?}")
        };
        assert_eq!(image.src, "img/logo.png");
        assert_eq!(image.alt_text(), "a logo");
        assert_eq!(image.title.as_deref(), Some("The logo"));
        assert_eq!(image.attr.get("width"), Some("50%"));
        assert!(image.attr.has_class("wide"));
        assert_eq!(p[2], text(" here."));
    }

    #[test]
    fn an_image_alone_is_a_figure_with_its_alt_as_caption() {
        let blocks = parse("![The *caption*](chart.png){#fig:c width=80%}\n");
        let Block::Figure(image) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(image.src, "chart.png");
        assert_eq!(
            image.alt,
            vec![text("The "), Inline::Emph(vec![text("caption")])]
        );
        assert_eq!(image.attr.id.as_deref(), Some("fig:c"));
    }

    #[test]
    fn an_image_without_alt_alone_is_not_a_figure() {
        let blocks = parse("![](https://example.invalid/x.png)\n");
        assert!(matches!(&blocks[0], Block::Paragraph(p) if matches!(p[0], Inline::Image(_))));
    }

    #[test]
    fn a_data_uri_image_is_kept_verbatim() {
        let p = para("x ![dot](data:image/png;base64,iVBORw0KGgo=) y");
        let Inline::Image(image) = &p[1] else {
            panic!()
        };
        assert_eq!(image.src, "data:image/png;base64,iVBORw0KGgo=");
    }

    #[test]
    fn links_keep_url_title_and_attributes() {
        let p = para("[the *site*](https://deraison.ai \"home\"){.ext} and <https://a.b>");
        let Inline::Link {
            text: t,
            url,
            title,
            attr,
        } = &p[0]
        else {
            panic!("{p:?}")
        };
        assert_eq!(t, &vec![text("the "), Inline::Emph(vec![text("site")])]);
        assert_eq!(url, "https://deraison.ai");
        assert_eq!(title.as_deref(), Some("home"));
        assert!(attr.has_class("ext"));
        assert!(matches!(&p[2], Inline::Link { url, .. } if url == "https://a.b"));
    }

    // ── notes ──

    #[test]
    fn footnotes_reference_and_definition() {
        let blocks = parse("Text[^1].\n\n[^1]: The note.\n");
        assert_eq!(
            blocks[0],
            Block::Paragraph(vec![
                text("Text"),
                Inline::FootnoteRef("1".into()),
                text(".")
            ])
        );
        assert!(matches!(&blocks[1], Block::FootnoteDef { label, .. } if label == "1"));
    }

    #[test]
    fn inline_notes_span_formatting_and_nested_brackets() {
        let p = para("Claim^[see *this* [1] too] and more^[two].");
        assert_eq!(p[0], text("Claim"));
        assert_eq!(
            p[1],
            Inline::InlineNote(vec![
                text("see "),
                Inline::Emph(vec![text("this")]),
                text(" [1] too")
            ])
        );
        assert_eq!(p[2], text(" and more"));
        assert_eq!(p[3], Inline::InlineNote(vec![text("two")]));
        assert_eq!(p[4], text("."));
    }

    #[test]
    fn superscript_still_works_beside_inline_notes() {
        let p = para("2^10^ is big^[a note].");
        assert_eq!(p[0], text("2"));
        assert_eq!(p[1], Inline::Superscript(vec![text("10")]));
        assert!(matches!(p[3], Inline::InlineNote(_)));
    }

    #[test]
    fn a_caret_bracket_in_code_is_untouched() {
        let p = para("`x^[0]` and an unmatched ^[ here");
        assert_eq!(p[0], Inline::Code("x^[0]".into()));
        assert_eq!(p[1], text(" and an unmatched ^[ here"));
    }

    #[test]
    fn subscript_and_strikeout() {
        let p = para("H~2~O ~~gone~~ and a ~sub~ word");
        assert_eq!(p[1], Inline::Subscript(vec![text("2")]));
        assert_eq!(p[2], text("O "));
        assert_eq!(p[3], Inline::Strikeout(vec![text("gone")]));
        assert_eq!(p[5], Inline::Subscript(vec![text("sub")]));
    }

    #[test]
    fn carets_and_tildes_with_spaces_stay_text() {
        let p = para("about ~ 5 km, x ^ y");
        assert_eq!(p, vec![text("about ~ 5 km, x ^ y")]);
    }

    // ── math ──

    #[test]
    fn inline_and_display_math_keep_raw_latex() {
        let p = para("Inline $E = mc^2$ and $$\\frac{a}{b}$$.");
        assert_eq!(
            p[1],
            Inline::Math {
                display: false,
                tex: "E = mc^2".into()
            }
        );
        assert_eq!(
            p[3],
            Inline::Math {
                display: true,
                tex: "\\frac{a}{b}".into()
            }
        );
    }

    #[test]
    fn multi_line_display_math() {
        let p = para("$$\ne^{i \\pi}+1 = 0\n$$\n");
        assert!(
            matches!(&p[0], Inline::Math { display: true, tex } if tex.contains("e^{i \\pi}+1 = 0")),
            "{p:?}"
        );
    }

    #[test]
    fn math_in_a_code_span_is_math() {
        let p = para("`$x^2$` and `quality_threshold $\\in [0,1]$` and `plain_code`");
        assert_eq!(
            p[0],
            Inline::Math {
                display: false,
                tex: "x^2".into()
            }
        );
        assert_eq!(
            p[2],
            Inline::Math {
                display: false,
                tex: "\\text{quality threshold} \\in [0,1]".into()
            }
        );
        assert_eq!(p[4], Inline::Code("plain_code".into()));
    }

    // ── citations ──

    #[test]
    fn bracketed_citations_with_locators_and_prefixes() {
        let p = para("As shown [see @pearl2009, p. 3; -@marr1982] here.");
        assert_eq!(
            p[1],
            Inline::Cite(vec![
                Citation {
                    key: "pearl2009".into(),
                    prefix: "see".into(),
                    suffix: "p. 3".into(),
                    mode: CitationMode::Normal
                },
                Citation {
                    key: "marr1982".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    mode: CitationMode::SuppressAuthor
                },
            ])
        );
    }

    #[test]
    fn in_text_citations_and_their_locator() {
        let p = para("@pearl2009 [p. 33] says so, as does @causality-pearl.");
        assert_eq!(
            p[0],
            Inline::Cite(vec![Citation {
                key: "pearl2009".into(),
                prefix: String::new(),
                suffix: "p. 33".into(),
                mode: CitationMode::AuthorInText
            }])
        );
        assert_eq!(p[1], text(" says so, as does "));
        assert!(matches!(&p[2], Inline::Cite(c) if c[0].key == "causality-pearl"));
        assert_eq!(p[3], text("."));
    }

    #[test]
    fn emails_and_plain_brackets_are_not_citations() {
        let p = para("Write to a@b.com [not a cite] now.");
        assert_eq!(p, vec![text("Write to a@b.com [not a cite] now.")]);
    }

    // ── tables ──

    #[test]
    fn tables_keep_alignments_and_a_following_caption() {
        let blocks =
            parse("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n\nTable: Results {#tbl:r}\n");
        assert_eq!(
            blocks.len(),
            1,
            "the caption paragraph is absorbed: {blocks:?}"
        );
        let Block::Table {
            headers,
            rows,
            aligns,
            caption,
            attr,
        } = &blocks[0]
        else {
            panic!()
        };
        assert_eq!(headers.len(), 3);
        assert_eq!(rows.len(), 1);
        assert_eq!(aligns, &vec![Align::Left, Align::Center, Align::Right]);
        assert_eq!(caption, &vec![text("Results")]);
        assert_eq!(attr.id.as_deref(), Some("tbl:r"));
    }

    #[test]
    fn a_colon_caption_before_the_table() {
        let blocks = parse(": Before\n\n| a |\n|---|\n| 1 |\n");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], Block::Table { caption, .. } if caption == &vec![text("Before")])
        );
    }

    #[test]
    fn a_caption_glued_to_the_table_is_not_a_row() {
        let blocks = parse("| a | b |\n|---|---|\n| 1 | 2 |\nTable: Glued\n");
        let Block::Table { rows, caption, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(caption, &vec![text("Glued")]);
    }

    // ── lists ──

    #[test]
    fn task_list_items_carry_their_state() {
        let blocks = parse("- [x] done\n- [ ] todo\n- plain\n");
        let Block::List { items, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(items[0].task, Some(true));
        assert_eq!(items[1].task, Some(false));
        assert_eq!(items[2].task, None);
        assert_eq!(items[0].content, vec![Block::Paragraph(vec![text("done")])]);
    }

    #[test]
    fn a_nested_task_does_not_steal_its_parents_state() {
        let blocks = parse("- [x] parent\n  - [ ] child\n");
        let Block::List { items, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(items[0].task, Some(true));
        let Block::List { items: inner, .. } = &items[0].content[1] else {
            panic!("{:?}", items[0].content)
        };
        assert_eq!(inner[0].task, Some(false));
    }

    #[test]
    fn ordered_list_start() {
        let blocks = parse("5. five\n6. six\n");
        assert!(
            matches!(&blocks[0], Block::List { ordered: true, start: 5, items } if items.len() == 2)
        );
    }

    // ── slides, notes, divs ──

    #[test]
    fn slide_separators_are_thematic_breaks() {
        let blocks = parse("# A\n\n---\n\n# B\n\n***\n\n# C\n");
        let breaks = blocks
            .iter()
            .filter(|b| matches!(b, Block::ThematicBreak))
            .count();
        assert_eq!(breaks, 2);
    }

    #[test]
    fn speaker_notes_are_their_own_block() {
        let blocks = parse("# Slide\n\nShown.\n::: notes\nSay *this*.\n:::\n");
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_eq!(
            blocks[2],
            Block::Notes(vec![Block::Paragraph(vec![
                text("Say "),
                Inline::Emph(vec![text("this")]),
                text(".")
            ])])
        );
    }

    #[test]
    fn braced_notes_class_is_notes_too() {
        let blocks = parse(":::: {.notes}\nhidden\n::::\n");
        assert!(matches!(&blocks[0], Block::Notes(_)));
    }

    #[test]
    fn nested_column_divs() {
        let blocks = parse(
            "::: columns\n:::: column\nLeft *a*.\n::::\n\n:::: {.column width=\"40%\"}\nRight.\n::::\n:::\n",
        );
        let Block::Div { attr, content } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert!(attr.has_class("columns"));
        assert_eq!(content.len(), 2);
        let Block::Div { attr: right, .. } = &content[1] else {
            panic!()
        };
        assert!(right.has_class("column"));
        assert_eq!(right.get("width"), Some("40%"));
    }

    #[test]
    fn a_div_fence_inside_code_is_code() {
        let blocks = parse("```\n::: notes\n:::\n```\n");
        assert!(matches!(&blocks[0], Block::CodeBlock { code, .. } if code == "::: notes\n:::\n"));
    }

    #[test]
    fn an_unclosed_div_is_closed_with_a_warning() {
        let doc = read("::: warning\nCareful.\n");
        assert!(doc.warnings.iter().any(|w| w.code == "unclosed-div"));
        assert!(matches!(&doc.blocks[0], Block::Div { content, .. } if content.len() == 1));
    }

    #[test]
    fn a_stray_closing_fence_is_text() {
        let blocks = parse("Before\n\n:::\n");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1], Block::Paragraph(vec![text(":::")]));
    }

    // ── code and mermaid ──

    #[test]
    fn code_info_strings() {
        let blocks =
            parse("```python {.numberLines #c1}\nx = 1\n```\n\n```{.rust}\nfn f() {}\n```\n");
        assert!(
            matches!(&blocks[0], Block::CodeBlock { lang: Some(l), attr, .. }
            if l == "python" && attr.has_class("numberLines") && attr.id.as_deref() == Some("c1"))
        );
        assert!(matches!(&blocks[1], Block::CodeBlock { lang: Some(l), .. } if l == "rust"));
    }

    #[test]
    fn mermaid_blocks_are_recognised() {
        let blocks = parse("```mermaid\nflowchart LR\n  a --> b\n```\n\n```{.mermaid}\npie\n```\n");
        assert!(matches!(&blocks[0], Block::Mermaid { code, .. } if code.starts_with("flowchart")));
        assert!(matches!(&blocks[1], Block::Mermaid { .. }));
    }

    #[test]
    fn raw_html_blocks_and_comments_are_kept() {
        let blocks = parse("<!--\nspeaker comment\n-->\n\nText <br> more\n");
        assert!(matches!(&blocks[0], Block::RawHtml(h) if h.contains("speaker comment")));
        let Block::Paragraph(p) = &blocks[1] else {
            panic!()
        };
        assert!(p.contains(&Inline::RawHtml("<br>".into())));
    }

    #[test]
    fn block_quotes_nest() {
        let blocks = parse("> quoted\n>\n> > deeper\n");
        let Block::BlockQuote(inner) = &blocks[0] else {
            panic!()
        };
        assert!(matches!(inner[1], Block::BlockQuote(_)));
    }

    #[test]
    fn adjacent_text_is_merged() {
        // pulldown-cmark splits text at brackets; the reader must hand writers one run.
        let p = para("a [b] c");
        assert_eq!(p, vec![text("a [b] c")]);
    }
}
