//! Markdown emission shared by the DOCX and PPTX readers: formatted runs → inline
//! Markdown with minimal, correct escaping; tables; footnote definitions.
//!
//! The readers do not build a [`crate::ast::Document`]: what they recover is flatter
//! than the AST (a run is bold or not, a cell is one line), and writing Markdown text
//! directly keeps the twin readable — the goal is a file a person edits, which the
//! reader then parses like any other.

use crate::ast::Align;

/// Character formatting of a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fmt {
    /// `**x**`.
    pub bold: bool,
    /// `*x*`.
    pub italic: bool,
    /// `` `x` ``.
    pub code: bool,
    /// `~~x~~`.
    pub strike: bool,
    /// `^x^`.
    pub sup: bool,
    /// `~x~`.
    pub sub: bool,
}

/// A piece of inline content as read from a document.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// Text with its formatting (not yet escaped).
    Text(String, Fmt),
    /// Markdown emitted verbatim (a footnote reference, an image, a formula).
    Raw(String),
    /// A hard line break.
    Break,
    /// A hyperlink around more pieces.
    Link(String, Vec<Piece>),
}

/// Render pieces to inline Markdown. `in_table` escapes pipes and turns breaks into
/// spaces (a pipe-table cell is one line).
pub fn inline(pieces: &[Piece], in_table: bool) -> String {
    let mut out = String::new();
    let mut runs: Vec<(String, Fmt)> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(t, f) => {
                if t.is_empty() {
                    continue;
                }
                match runs.last_mut() {
                    Some((prev, pf)) if pf == f => prev.push_str(t),
                    _ => runs.push((t.clone(), *f)),
                }
            }
            other => {
                flush(&mut runs, &mut out, in_table);
                match other {
                    Piece::Raw(r) => out.push_str(r),
                    Piece::Break => {
                        if in_table {
                            out.push(' ');
                        } else {
                            out.push_str("\\\n");
                        }
                    }
                    Piece::Link(url, inner) => {
                        let text = inline(inner, in_table);
                        if text.trim().is_empty() {
                            continue;
                        }
                        if text == url.as_str() || text == format!("mailto:{url}") {
                            out.push('<');
                            out.push_str(url);
                            out.push('>');
                        } else {
                            out.push('[');
                            out.push_str(&text);
                            out.push_str("](");
                            out.push_str(&link_target(url));
                            out.push(')');
                        }
                    }
                    Piece::Text(..) => {}
                }
            }
        }
    }
    flush(&mut runs, &mut out, in_table);
    out
}

/// A link destination, wrapped in `<…>` when it holds spaces or parentheses.
pub fn link_target(url: &str) -> String {
    if url.contains([' ', '(', ')']) {
        format!("<{}>", url.replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

/// Emit merged runs with balanced markers. Leading/trailing whitespace stays outside
/// the markers (CommonMark's flanking rules would otherwise not close them).
fn flush(runs: &mut Vec<(String, Fmt)>, out: &mut String, in_table: bool) {
    // Split each run's outer whitespace into unformatted runs.
    let mut split: Vec<(String, Fmt)> = Vec::new();
    for (text, fmt) in runs.drain(..) {
        if fmt == Fmt::default() || fmt.code {
            split.push((text, fmt));
            continue;
        }
        let trimmed_start = text.trim_start();
        let lead = &text[..text.len() - trimmed_start.len()];
        let core = trimmed_start.trim_end();
        let trail = &trimmed_start[core.len()..];
        if !lead.is_empty() {
            split.push((lead.to_string(), Fmt::default()));
        }
        if !core.is_empty() {
            split.push((core.to_string(), fmt));
        }
        if !trail.is_empty() {
            split.push((trail.to_string(), Fmt::default()));
        }
    }
    // Merge again after the split.
    let mut merged: Vec<(String, Fmt)> = Vec::new();
    for (t, f) in split {
        match merged.last_mut() {
            Some((prev, pf)) if *pf == f => prev.push_str(&t),
            _ => merged.push((t, f)),
        }
    }
    // Marker stack: bold outermost, then italic, then strike; sup/sub/code are atomic.
    let mut open: Vec<&'static str> = Vec::new();
    let mut prev_tail = String::new();
    for (text, fmt) in &merged {
        let want: Vec<&'static str> = [(fmt.bold, "**"), (fmt.italic, "*"), (fmt.strike, "~~")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, m)| *m)
            .collect();
        // Keep the longest common prefix of open markers, close the rest.
        let keep = open.iter().zip(&want).take_while(|(a, b)| a == b).count();
        while open.len() > keep {
            if let Some(m) = open.pop() {
                out.push_str(m);
            }
        }
        for m in &want[keep..] {
            out.push_str(m);
            open.push(m);
        }
        let body = if fmt.code {
            code_span(text)
        } else {
            let escaped = escape(text, in_table, &prev_tail);
            if fmt.sup {
                format!("^{}^", escaped.replace(' ', "\\ "))
            } else if fmt.sub {
                format!("~{}~", escaped.replace(' ', "\\ "))
            } else {
                escaped
            }
        };
        out.push_str(&body);
        prev_tail = text.chars().rev().take(1).collect();
    }
    while let Some(m) = open.pop() {
        out.push_str(m);
    }
}

/// A code span whose fence is longer than any backtick run inside.
pub fn code_span(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = text.starts_with('`') || text.ends_with('`');
    if pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// Escape text so the Markdown reader gives it back verbatim. Only what would change
/// meaning is escaped: emphasis and code markers, brackets, a `$` that could open a
/// formula, an `@` that could start a citation, a `<` that could open HTML.
pub fn escape(text: &str, in_table: bool, before: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut prev = before.chars().last();
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let needs = match c {
            '\\' | '*' | '_' | '`' | '[' | ']' | '~' | '^' => {
                // `_` inside a word is literal in CommonMark; keep `snake_case` clean.
                !(c == '_'
                    && prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric))
            }
            '$' => next.is_some_and(|n| !n.is_whitespace() && !n.is_ascii_digit()),
            '@' => {
                !prev.is_some_and(|p| p.is_alphanumeric())
                    && next.is_some_and(|n| n.is_alphanumeric() || n == '_')
            }
            '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')),
            '|' => in_table,
            '&' => {
                // `&amp;` / `&#233;` would be decoded as an entity.
                let rest: String = chars[i + 1..].iter().take(12).collect();
                rest.find(';').is_some_and(|j| {
                    j > 0
                        && rest[..j]
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '#')
                })
            }
            _ => false,
        };
        if needs {
            out.push('\\');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Escape what would turn the start of a paragraph into another block (a heading, a
/// list item, a quote, a fence, a table, a thematic break).
pub fn guard_line_start(line: &str) -> String {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next();
    let block_marker = match first {
        Some('#') | Some('>') | Some('|') | Some('=') => true,
        Some('-') | Some('+') => {
            trimmed.len() == 1
                || trimmed[1..].starts_with(' ')
                || trimmed.chars().all(|c| c == '-' || c == ' ')
        }
        Some(':') => trimmed.starts_with(":::"),
        _ => false,
    };
    if block_marker {
        return format!("\\{trimmed}");
    }
    // `1. ` / `1) ` → `1\. `
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() && digits.len() <= 9 {
        let rest = &trimmed[digits.len()..];
        if (rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")")
            && !rest.is_empty()
        {
            return format!("{digits}\\{rest}");
        }
    }
    trimmed.to_string()
}

/// A GFM pipe table. The first row is the header; `aligns` may be shorter than the
/// widest row (missing columns align by default).
pub fn table(rows: &[Vec<String>], aligns: &[Align]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let line = |cells: &[String]| {
        let mut s = String::from("|");
        for i in 0..width {
            let cell = cells.get(i).map(String::as_str).unwrap_or("");
            s.push(' ');
            s.push_str(cell);
            s.push_str(" |");
        }
        s
    };
    let mut out = line(&rows[0]);
    out.push('\n');
    out.push('|');
    for i in 0..width {
        out.push_str(match aligns.get(i).copied().unwrap_or_default() {
            Align::Default => "---|",
            Align::Left => ":---|",
            Align::Center => ":---:|",
            Align::Right => "---:|",
        });
    }
    for row in &rows[1..] {
        out.push('\n');
        out.push_str(&line(row));
    }
    out
}

/// A footnote definition: `[^label]: first block`, later blocks indented four spaces.
pub fn footnote(label: &str, blocks: &[String]) -> String {
    let mut out = format!("[^{label}]:");
    for (i, block) in blocks.iter().enumerate() {
        if i == 0 {
            out.push(' ');
            out.push_str(&indent(block, 4, false));
        } else {
            out.push_str("\n\n");
            out.push_str(&indent(block, 4, true));
        }
    }
    out
}

/// Indent every line of `text` by `n` spaces (the first too when `first`).
pub fn indent(text: &str, n: usize, first: bool) -> String {
    let pad = " ".repeat(n);
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if (i > 0 || first) && !line.is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    out
}

/// A fenced code block whose fence is longer than any backtick run in the code.
pub fn code_block(lang: &str, code: &str) -> String {
    let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{lang}\n{code}\n{fence}")
}

/// Prefix every line with `> ` (blank lines get a bare `>`).
pub fn quote(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            if l.is_empty() {
                ">".into()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One YAML scalar, double-quoted when plain style could misread it.
pub fn yaml_scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with(|c: char| "!&*-?{}[],#|>@`\"'%:= ".contains(c))
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.ends_with([':', ' '])
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
        )
        && s.parse::<f64>().is_err();
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str, f: Fmt) -> Piece {
        Piece::Text(s.into(), f)
    }

    const B: Fmt = Fmt {
        bold: true,
        italic: false,
        code: false,
        strike: false,
        sup: false,
        sub: false,
    };
    const I: Fmt = Fmt {
        bold: false,
        italic: true,
        code: false,
        strike: false,
        sup: false,
        sub: false,
    };

    #[test]
    fn merges_runs_and_keeps_whitespace_outside_markers() {
        let p = [
            t("a ", Fmt::default()),
            t("bold ", B),
            t("text", B),
            t(" end", Fmt::default()),
        ];
        assert_eq!(inline(&p, false), "a **bold text** end");
    }

    #[test]
    fn nests_bold_and_italic() {
        let bi = Fmt { italic: true, ..B };
        let p = [t("x", B), t("y", bi), t("z", B), t(" w", I)];
        assert_eq!(inline(&p, false), "**x*y*z** *w*");
    }

    #[test]
    fn escapes_only_what_changes_meaning() {
        assert_eq!(
            escape("snake_case and 2*3 [x]", false, ""),
            "snake_case and 2\\*3 \\[x\\]"
        );
        assert_eq!(
            escape("costs $5, not $x$", false, ""),
            "costs $5, not \\$x$"
        );
        assert_eq!(
            escape("mail a@b.c, cite @key", false, ""),
            "mail a@b.c, cite \\@key"
        );
        assert_eq!(escape("a|b", true, ""), "a\\|b");
        assert_eq!(escape("1 < 2 <b>", false, ""), "1 < 2 \\<b>");
    }

    #[test]
    fn guards_block_markers_at_line_start() {
        assert_eq!(guard_line_start("# not a heading"), "\\# not a heading");
        assert_eq!(guard_line_start("1. not a list"), "1\\. not a list");
        assert_eq!(guard_line_start("- not a bullet"), "\\- not a bullet");
        assert_eq!(guard_line_start("-5 degrees"), "-5 degrees");
    }

    #[test]
    fn code_spans_outrun_inner_backticks() {
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("x"), "`x`");
    }

    #[test]
    fn links_and_autolinks() {
        let p = [Piece::Link(
            "https://x.org".into(),
            vec![t("https://x.org", Fmt::default())],
        )];
        assert_eq!(inline(&p, false), "<https://x.org>");
        let p = [Piece::Link(
            "https://x.org/a b".into(),
            vec![t("here", Fmt::default())],
        )];
        assert_eq!(inline(&p, false), "[here](<https://x.org/a b>)");
    }

    #[test]
    fn tables_and_footnotes() {
        let rows = vec![vec!["A".into(), "B".into()], vec!["1".into()]];
        assert_eq!(
            table(&rows, &[Align::Left, Align::Right]),
            "| A | B |\n|:---|---:|\n| 1 |  |"
        );
        assert_eq!(
            footnote("1", &["one".into(), "two\nlines".into()]),
            "[^1]: one\n\n    two\n    lines"
        );
    }

    #[test]
    fn yaml_scalars_are_quoted_when_ambiguous() {
        assert_eq!(yaml_scalar("Plain title"), "Plain title");
        assert_eq!(yaml_scalar("A: B"), "\"A: B\"");
        assert_eq!(yaml_scalar("2026"), "\"2026\"");
        assert_eq!(yaml_scalar("yes"), "\"yes\"");
    }
}
