//! LaTeX → OMML: native Word (and PowerPoint) equations from the raw LaTeX the reader
//! keeps in [`crate::ast::Inline::Math`].
//!
//! A small recursive-descent parser turns the common subset of LaTeX math into a tree,
//! which is then written as Office Math Markup Language. The subset, and the OMML each
//! construction becomes (the same structures Pandoc's texmath writes, in the element
//! order the OMML schema asks for):
//!
//! | LaTeX | OMML |
//! |---|---|
//! | `\frac`, `\dfrac`, `\tfrac`, `\cfrac`; `\binom` | `m:f` (`bar`; `noBar` inside `m:d`) |
//! | `x_i`, `x^2`, `x_i^2`, `f'` | `m:sSub`, `m:sSup`, `m:sSubSup` |
//! | `\sqrt{x}`, `\sqrt[n]{x}` | `m:rad` |
//! | `\sum`, `\prod`, `\int`, `\iint`, `\oint`, `\bigcup`… with bounds | `m:nary` |
//! | `\lim`, `\max`, `\min`, `\sup`, `\inf`, `\operatorname*` with a subscript | `m:func` + `m:limLow` |
//! | `\left( … \middle| … \right)`, matched `( )` and `[ ]`, `\binom` | `m:d` |
//! | `matrix`, `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`, `cases`, `array`, `aligned`, `align`, `gathered`, `split` | `m:m` (in `m:d` for the bracketed ones) |
//! | `\hat`, `\bar`, `\vec`, `\tilde`, `\dot`, `\ddot`, `\check`, `\breve`, `\widehat`… | `m:acc` |
//! | `\overline`, `\underline` | `m:bar` |
//! | `\overbrace`, `\underbrace` | `m:groupChr` (+ `m:limUpp`/`m:limLow`) |
//! | `\overset`, `\underset`, `\stackrel`, `\xrightarrow` | `m:limUpp`, `m:limLow` |
//! | `\boxed` | `m:borderBox` |
//! | `\mathbf`, `\mathrm`, `\mathit`, `\mathbb`, `\mathcal`, `\mathfrak`, `\mathsf`, `\mathtt`, `\boldsymbol` | `m:sty` / `m:scr` on the runs |
//! | `\text`, `\mbox`, `\textrm`, `\operatorname` | a normal-text (`m:nor`) or upright run |
//! | Greek letters, operators, relations, arrows, `\sin`… | runs (upright for operators) |
//!
//! Inline math becomes an `<m:oMath>` placed in the paragraph; display math an
//! `<m:oMathPara>`. The fragment uses the `m:` prefix without declaring it: the
//! enclosing part declares `xmlns:m` (the DOCX writer does on `w:document`; the PPTX
//! writer wraps it in `a14:m` with its own declaration).
//!
//! Outside the subset, nothing fails: an unknown command becomes its own name as upright
//! text in the equation, plus a warning ([`convert`]). Only [`latex_to_omml`], the strict
//! entry point, refuses input it cannot read (unbalanced braces).
//!
//! Owned by the DOCX port (see [`crate::docx`]); the PPTX writer calls it too.

use crate::ast::Warning;
use crate::error::{Error, Result};

/// The result of [`convert`]: the OMML fragment and what could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omml {
    /// `<m:oMath>…</m:oMath>` or `<m:oMathPara>…</m:oMathPara>`.
    pub xml: String,
    /// One warning per construction outside the supported subset (code `math-unsupported`).
    pub warnings: Vec<Warning>,
}

/// Convert LaTeX math source (without its `$` delimiters) to an OMML fragment:
/// `<m:oMathPara>…</m:oMathPara>` when `display`, `<m:oMath>…</m:oMath>` otherwise.
///
/// Unknown commands degrade to their name as text (see [`convert`] for the warnings).
///
/// # Errors
///
/// [`Error::Math`] when the braces of `tex` are unbalanced.
///
/// # Examples
///
/// ```
/// let xml = md2star_rs::docx::math::latex_to_omml(r"\frac{a}{b}", false).unwrap();
/// assert!(xml.starts_with("<m:oMath><m:f>"));
/// assert!(md2star_rs::docx::math::latex_to_omml(r"\frac{a}{b", false).is_err());
/// ```
pub fn latex_to_omml(tex: &str, display: bool) -> Result<String> {
    let mut depth = 0i64;
    let mut escaped = false;
    for c in tex.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(Error::Math(format!("unbalanced `}}` in `{tex}`")));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(Error::Math(format!("unbalanced `{{` in `{tex}`")));
    }
    Ok(convert(tex, display).xml)
}

/// Convert LaTeX math to OMML, never failing: what the converter does not know becomes
/// readable text inside the equation, and a warning says what it was.
///
/// # Examples
///
/// ```
/// let out = md2star_rs::docx::math::convert(r"x \foo y", false);
/// assert!(out.xml.contains(r"\foo"));
/// assert_eq!(out.warnings[0].code, "math-unsupported");
/// ```
pub fn convert(tex: &str, display: bool) -> Omml {
    let mut parser = Parser {
        chars: tex.chars().collect(),
        pos: 0,
        warnings: Vec::new(),
        source: tex,
    };
    let rows = parser.top();
    let body = if rows.len() > 1 {
        let mut s = String::from("<m:eqArr>");
        for row in &rows {
            s.push_str("<m:e>");
            write_nodes(row, &mut s);
            s.push_str("</m:e>");
        }
        s.push_str("</m:eqArr>");
        s
    } else {
        let mut s = String::new();
        if let Some(row) = rows.first() {
            write_nodes(row, &mut s);
        }
        s
    };
    let xml = if display {
        format!(
            "<m:oMathPara><m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>\
             <m:oMath>{body}</m:oMath></m:oMathPara>"
        )
    } else {
        format!("<m:oMath>{body}</m:oMath>")
    };
    Omml {
        xml,
        warnings: parser.warnings,
    }
}

/// How a run is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RunStyle {
    /// `m:sty`: `p` (upright), `b`, `i`, `bi`; `None` = Word's default (italic letters).
    sty: Option<&'static str>,
    /// `m:scr`: `double-struck`, `script`, `fraktur`, `sans-serif`, `monospace`.
    scr: Option<&'static str>,
    /// `m:nor`: normal (non-math) text.
    nor: bool,
}

const UPRIGHT: RunStyle = RunStyle {
    sty: Some("p"),
    scr: None,
    nor: false,
};

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Run(String, RunStyle),
    Group(Vec<Node>),
    Frac {
        num: Vec<Node>,
        den: Vec<Node>,
        bar: bool,
    },
    Scripts {
        base: Vec<Node>,
        sub: Option<Vec<Node>>,
        sup: Option<Vec<Node>>,
    },
    Rad {
        deg: Option<Vec<Node>>,
        body: Vec<Node>,
    },
    Nary {
        chr: char,
        under_over: bool,
        sub: Option<Vec<Node>>,
        sup: Option<Vec<Node>>,
        body: Vec<Node>,
    },
    Func {
        name: Vec<Node>,
        arg: Vec<Node>,
    },
    Lim {
        upper: bool,
        base: Vec<Node>,
        lim: Vec<Node>,
    },
    Delim {
        beg: String,
        end: String,
        sep: String,
        items: Vec<Vec<Node>>,
    },
    Matrix {
        rows: Vec<Vec<Vec<Node>>>,
        jcs: Vec<&'static str>,
    },
    Acc {
        chr: char,
        body: Vec<Node>,
    },
    Bar {
        top: bool,
        body: Vec<Node>,
    },
    GroupChr {
        chr: char,
        top: bool,
        body: Vec<Node>,
    },
    BorderBox(Vec<Node>),
}

/// What ends a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// End of input only (also `\\` at the top level, which splits rows).
    Top,
    /// `}`.
    Brace,
    /// `]` (an optional argument).
    Bracket,
    /// A plain closing character, for matched `( )` / `[ ]`.
    Char(char),
    /// `\middle` or `\right`.
    Right,
    /// `&`, `\\` or `\end` (inside an environment).
    Cell,
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    warnings: Vec<Warning>,
    source: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    /// Does the input at the current position start with `s`?
    fn at(&self, s: &str) -> bool {
        (self.pos..)
            .zip(s.chars())
            .all(|(i, c)| self.chars.get(i) == Some(&c))
    }

    /// Is the input at a control word `\name` (not followed by another letter)?
    fn at_command(&self, name: &str) -> bool {
        self.at(&format!("\\{name}"))
            && !self
                .chars
                .get(self.pos + 1 + name.chars().count())
                .is_some_and(char::is_ascii_alphabetic)
    }

    fn warn(&mut self, what: &str) {
        self.warnings.push(Warning::new(
            "math-unsupported",
            format!(
                "math: {what} in `{}` is not converted to an equation structure; kept as text",
                self.source.trim()
            ),
        ));
    }

    /// The whole input, split into rows on top-level `\\`.
    fn top(&mut self) -> Vec<Vec<Node>> {
        let mut rows = Vec::new();
        loop {
            let row = self.seq(Stop::Top);
            rows.push(row);
            if self.at("\\\\") {
                self.pos += 2;
                self.skip_optional_length();
                continue;
            }
            if self.pos >= self.chars.len() {
                break;
            }
            // A stray closing brace at the top: skip it.
            self.pos += 1;
        }
        rows
    }

    /// Skip `[2pt]` after `\\`.
    fn skip_optional_length(&mut self) {
        self.skip_ws();
        if self.peek() == Some('[') {
            while let Some(c) = self.peek() {
                self.pos += 1;
                if c == ']' {
                    break;
                }
            }
        }
    }

    fn at_stop(&self, stop: Stop) -> bool {
        match self.peek() {
            None => true,
            Some(c) => match stop {
                Stop::Top => c == '}' || self.at("\\\\"),
                Stop::Brace => c == '}',
                Stop::Bracket => c == ']' || c == '}',
                Stop::Char(close) => {
                    c == close
                        || c == '}'
                        || c == '&'
                        || self.at("\\\\")
                        || self.at_command("right")
                        || self.at_command("middle")
                        || self.at_command("end")
                }
                Stop::Right => c == '}' || self.at_command("right") || self.at_command("middle"),
                Stop::Cell => c == '&' || c == '}' || self.at("\\\\") || self.at_command("end"),
            },
        }
    }

    /// A sequence of atoms (with their scripts) up to `stop`, which is not consumed.
    fn seq(&mut self, stop: Stop) -> Vec<Node> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.at_stop(stop) {
                break;
            }
            // `&` outside an environment: an alignment point we ignore.
            if self.peek() == Some('&') {
                self.pos += 1;
                continue;
            }
            let before = self.pos;
            if let Some(node) = self.atom_with_scripts(true) {
                out.push(node);
            }
            if self.pos == before {
                // Never loop without progress.
                self.pos += 1;
            }
        }
        out
    }

    /// One atom, then any `_`, `^` and primes that follow it.
    fn atom_with_scripts(&mut self, greedy_number: bool) -> Option<Node> {
        self.skip_ws();
        let base = match self.peek() {
            Some('_' | '^' | '\'') => Node::Group(Vec::new()),
            _ => self.atom(greedy_number)?,
        };
        self.scripts(base)
    }

    /// Attach scripts to `base`.
    fn scripts(&mut self, base: Node) -> Option<Node> {
        let mut sub: Option<Vec<Node>> = None;
        let mut sup: Option<Vec<Node>> = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('_') if sub.is_none() => {
                    self.pos += 1;
                    sub = Some(self.argument());
                }
                Some('^') if sup.is_none() => {
                    self.pos += 1;
                    let arg = self.argument();
                    match &mut sup {
                        Some(primes) => primes.extend(arg),
                        None => sup = Some(arg),
                    }
                }
                Some('\'') => {
                    let mut primes = String::new();
                    while self.peek() == Some('\'') {
                        self.pos += 1;
                        primes.push('′');
                    }
                    let run = Node::Run(primes, UPRIGHT);
                    match &mut sup {
                        Some(existing) => existing.insert(0, run),
                        None => sup = Some(vec![run]),
                    }
                    // `f'^2`: a following ^ adds to the primes.
                    self.skip_ws();
                    if self.peek() == Some('^') {
                        self.pos += 1;
                        let arg = self.argument();
                        if let Some(existing) = &mut sup {
                            existing.extend(arg);
                        }
                    }
                }
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            return Some(base);
        }
        Some(match base {
            Node::Nary {
                chr,
                under_over,
                sub: s0,
                sup: p0,
                ..
            } => {
                let body = self.nary_body();
                Node::Nary {
                    chr,
                    under_over,
                    sub: sub.or(s0),
                    sup: sup.or(p0),
                    body,
                }
            }
            // `\lim_{x\to0} f`: a function whose name carries its limit underneath.
            Node::Func { name, arg } if arg.is_empty() && sub.is_some() && sup.is_none() => {
                let lim = sub.unwrap_or_default();
                let arg = self.nary_body();
                Node::Func {
                    name: vec![Node::Lim {
                        upper: false,
                        base: name,
                        lim,
                    }],
                    arg,
                }
            }
            Node::Func { name, .. } => Node::Scripts {
                base: name,
                sub,
                sup,
            },
            // `\overbrace{x}^{n}`, `\underbrace{x}_{n}`.
            Node::GroupChr { chr, top, body }
                if (top && sup.is_some()) || (!top && sub.is_some()) =>
            {
                let group = Node::GroupChr { chr, top, body };
                let lim = if top { sup } else { sub }.unwrap_or_default();
                Node::Lim {
                    upper: top,
                    base: vec![group],
                    lim,
                }
            }
            other => Node::Scripts {
                base: vec![other],
                sub,
                sup,
            },
        })
    }

    /// The operand of an n-ary operator or a limit: the next atom with its scripts.
    fn nary_body(&mut self) -> Vec<Node> {
        self.skip_ws();
        let stop_here = matches!(self.peek(), None | Some('}' | '&' | ']'))
            || self.at("\\\\")
            || self.at_command("right")
            || self.at_command("middle")
            || self.at_command("end");
        if stop_here {
            return Vec::new();
        }
        // An n-ary operator directly after another (`\sum\sum`) nests.
        self.atom_with_scripts(true).into_iter().collect()
    }

    /// A macro argument: `{…}` or a single token.
    fn argument(&mut self) -> Vec<Node> {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let inner = self.seq(Stop::Brace);
                if self.peek() == Some('}') {
                    self.pos += 1;
                }
                inner
            }
            None => Vec::new(),
            Some(_) => self.atom(false).into_iter().collect(),
        }
    }

    /// The raw text of a `{…}` argument (for `\text`, `\operatorname`, environment names).
    fn raw_argument(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self
                .peek()
                .map(|c| {
                    self.pos += 1;
                    c.to_string()
                })
                .unwrap_or_default();
        }
        self.pos += 1;
        let mut depth = 1;
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            s.push(c);
        }
        s
    }

    /// An optional `[…]` argument, parsed as math.
    fn optional_argument(&mut self) -> Option<Vec<Node>> {
        self.skip_ws();
        if self.peek() != Some('[') {
            return None;
        }
        self.pos += 1;
        let inner = self.seq(Stop::Bracket);
        if self.peek() == Some(']') {
            self.pos += 1;
        }
        Some(inner)
    }

    /// One atom (no scripts).
    fn atom(&mut self, greedy_number: bool) -> Option<Node> {
        self.skip_ws();
        let c = self.peek()?;
        match c {
            '{' => {
                self.pos += 1;
                let inner = self.seq(Stop::Brace);
                if self.peek() == Some('}') {
                    self.pos += 1;
                }
                Some(Node::Group(inner))
            }
            '\\' => self.command(),
            c if c.is_ascii_digit()
                || (c == '.'
                    && self
                        .chars
                        .get(self.pos + 1)
                        .is_some_and(char::is_ascii_digit)
                    && greedy_number) =>
            {
                let mut s = String::new();
                if greedy_number {
                    while let Some(d) = self.peek() {
                        let next_digit = self
                            .chars
                            .get(self.pos + 1)
                            .is_some_and(char::is_ascii_digit);
                        if d.is_ascii_digit() || (d == '.' && next_digit) {
                            s.push(d);
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                } else {
                    s.push(c);
                    self.pos += 1;
                }
                Some(Node::Run(s, RunStyle::default()))
            }
            c if c.is_alphabetic() => {
                self.pos += 1;
                Some(Node::Run(c.to_string(), RunStyle::default()))
            }
            '(' | '[' => {
                let close = if c == '(' { ')' } else { ']' };
                Some(self.matched(c, close))
            }
            '~' => {
                self.pos += 1;
                Some(Node::Run("\u{a0}".into(), RunStyle::default()))
            }
            '-' => {
                self.pos += 1;
                Some(Node::Run("−".into(), UPRIGHT))
            }
            '*' => {
                self.pos += 1;
                Some(Node::Run("∗".into(), UPRIGHT))
            }
            '}' | '&' => None,
            other => {
                self.pos += 1;
                Some(Node::Run(other.to_string(), UPRIGHT))
            }
        }
    }

    /// `(` … `)` with the same nesting level becomes a growing delimiter (as texmath
    /// does); an unmatched one stays a plain character.
    fn matched(&mut self, open: char, close: char) -> Node {
        let start = self.pos;
        let warnings = self.warnings.len();
        self.pos += 1;
        let inner = self.seq(Stop::Char(close));
        if self.peek() == Some(close) {
            self.pos += 1;
            return Node::Delim {
                beg: open.to_string(),
                end: close.to_string(),
                sep: String::new(),
                items: vec![inner],
            };
        }
        // Not matched here: rewind and emit the bare character.
        self.pos = start + 1;
        self.warnings.truncate(warnings);
        Node::Run(open.to_string(), UPRIGHT)
    }

    /// A delimiter after `\left`, `\right`, `\middle` or `\big…`: `(`, `\{`, `\langle`, `.`.
    fn delimiter(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.pos += 1;
                let name = self.command_name();
                match name.as_str() {
                    "{" | "lbrace" => "{".into(),
                    "}" | "rbrace" => "}".into(),
                    "|" | "Vert" | "lVert" | "rVert" => "‖".into(),
                    "vert" | "lvert" | "rvert" | "mid" => "|".into(),
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    "lfloor" => "⌊".into(),
                    "rfloor" => "⌋".into(),
                    "lceil" => "⌈".into(),
                    "rceil" => "⌉".into(),
                    "backslash" => "\\".into(),
                    "uparrow" => "↑".into(),
                    "downarrow" => "↓".into(),
                    other => {
                        self.warn(&format!("the delimiter `\\{other}`"));
                        format!("\\{other}")
                    }
                }
            }
            Some('.') => {
                self.pos += 1;
                String::new()
            }
            Some(c) => {
                self.pos += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    /// The name after a backslash: a run of letters, or one other character.
    fn command_name(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        if self.pos == start {
            if let Some(c) = self.peek() {
                self.pos += 1;
                return c.to_string();
            }
            return String::new();
        }
        // `\operatorname*`.
        let mut name: String = self.chars[start..self.pos].iter().collect();
        if self.peek() == Some('*') && matches!(name.as_str(), "operatorname") {
            self.pos += 1;
            name.push('*');
        }
        name
    }

    /// A backslash command.
    fn command(&mut self) -> Option<Node> {
        self.pos += 1;
        let name = self.command_name();
        let n = name.as_str();
        if let Some(sym) = symbol(n) {
            return Some(sym);
        }
        if let Some(chr) = nary(n) {
            let under_over = !matches!(chr, '∫' | '∬' | '∭' | '∮' | '∯' | '∰' | '⨌');
            return self.scripts_for_nary(chr, under_over);
        }
        if LIMIT_FUNCTIONS.contains(&n) {
            return Some(Node::Func {
                name: vec![Node::Run(n.to_string(), UPRIGHT)],
                arg: Vec::new(),
            });
        }
        if FUNCTIONS.contains(&n) {
            return Some(Node::Run(n.to_string(), UPRIGHT));
        }
        if let Some(chr) = accent(n) {
            let body = self.argument();
            return Some(Node::Acc { chr, body });
        }
        if let Some(style) = font(n) {
            let mut body = self.argument();
            restyle(&mut body, style);
            return Some(Node::Group(body));
        }
        match n {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.argument();
                let den = self.argument();
                Some(Node::Frac {
                    num,
                    den,
                    bar: true,
                })
            }
            "binom" | "dbinom" | "tbinom" => {
                let num = self.argument();
                let den = self.argument();
                Some(Node::Delim {
                    beg: "(".into(),
                    end: ")".into(),
                    sep: String::new(),
                    items: vec![vec![Node::Frac {
                        num,
                        den,
                        bar: false,
                    }]],
                })
            }
            "sqrt" => {
                let deg = self.optional_argument();
                let body = self.argument();
                Some(Node::Rad { deg, body })
            }
            "left" => Some(self.left_right()),
            "right" | "middle" => {
                // Unbalanced: show the delimiter.
                let d = self.delimiter();
                Some(Node::Run(d, UPRIGHT))
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" | "bigm" | "Bigm" => {
                let d = self.delimiter();
                Some(Node::Run(d, UPRIGHT))
            }
            "begin" => Some(self.environment()),
            "end" => {
                // A stray \end: drop its argument.
                self.raw_argument();
                None
            }
            "text" | "textrm" | "mbox" | "textnormal" | "textup" | "hbox" | "textsf" | "texttt" => {
                let text = self.raw_argument();
                Some(Node::Run(
                    text,
                    RunStyle {
                        nor: true,
                        ..RunStyle::default()
                    },
                ))
            }
            "textit" | "emph" => {
                let text = self.raw_argument();
                Some(Node::Run(
                    text,
                    RunStyle {
                        sty: Some("i"),
                        ..RunStyle::default()
                    },
                ))
            }
            "textbf" => {
                let text = self.raw_argument();
                Some(Node::Run(
                    text,
                    RunStyle {
                        sty: Some("b"),
                        ..RunStyle::default()
                    },
                ))
            }
            "operatorname" => {
                let text = self.raw_argument();
                Some(Node::Run(text.trim().to_string(), UPRIGHT))
            }
            "operatorname*" => {
                let text = self.raw_argument();
                Some(Node::Func {
                    name: vec![Node::Run(text.trim().to_string(), UPRIGHT)],
                    arg: Vec::new(),
                })
            }
            "overline" => {
                let body = self.argument();
                Some(Node::Bar { top: true, body })
            }
            "underline" => {
                let body = self.argument();
                Some(Node::Bar { top: false, body })
            }
            "overbrace" => {
                let body = self.argument();
                Some(Node::GroupChr {
                    chr: '⏞',
                    top: true,
                    body,
                })
            }
            "underbrace" => {
                let body = self.argument();
                Some(Node::GroupChr {
                    chr: '⏟',
                    top: false,
                    body,
                })
            }
            "overset" | "stackrel" => {
                let lim = self.argument();
                let base = self.argument();
                Some(Node::Lim {
                    upper: true,
                    base,
                    lim,
                })
            }
            "underset" => {
                let lim = self.argument();
                let base = self.argument();
                Some(Node::Lim {
                    upper: false,
                    base,
                    lim,
                })
            }
            "xrightarrow" | "xleftarrow" | "xRightarrow" | "xLeftarrow" => {
                let _below = self.optional_argument();
                let lim = self.argument();
                let arrow = match n {
                    "xrightarrow" => "⟶",
                    "xleftarrow" => "⟵",
                    "xRightarrow" => "⟹",
                    _ => "⟸",
                };
                Some(Node::Lim {
                    upper: true,
                    base: vec![Node::Run(arrow.into(), UPRIGHT)],
                    lim,
                })
            }
            "boxed" | "fbox" => {
                let body = self.argument();
                Some(Node::BorderBox(body))
            }
            "not" => {
                let next = self.atom(false)?;
                Some(negate(next))
            }
            "color" => {
                self.raw_argument();
                None
            }
            "textcolor" | "colorbox" => {
                self.raw_argument();
                let body = self.argument();
                Some(Node::Group(body))
            }
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" | "limits"
            | "nolimits" | "nonumber" | "notag" | "!" | "relax" | "strut" | "mathstrut"
            | "allowbreak" => None,
            "label" | "vspace" | "phantom" | "vphantom" | "hphantom" => {
                self.raw_argument();
                None
            }
            "hspace" => {
                self.raw_argument();
                Some(Node::Run(" ".into(), RunStyle::default()))
            }
            "tag" => {
                let text = self.raw_argument();
                Some(Node::Run(
                    format!("\u{2003}({text})"),
                    RunStyle {
                        nor: true,
                        ..RunStyle::default()
                    },
                ))
            }
            "\\" => None,
            "" => None,
            other => {
                self.warn(&format!("`\\{other}`"));
                Some(Node::Run(format!("\\{other}"), UPRIGHT))
            }
        }
    }

    /// An n-ary operator: its limits are attached by [`Parser::scripts`].
    fn scripts_for_nary(&mut self, chr: char, under_over: bool) -> Option<Node> {
        // `\limits` / `\nolimits` right after the operator.
        self.skip_ws();
        let mut under_over = under_over;
        if self.at_command("limits") {
            self.pos += 7;
            under_over = true;
        } else if self.at_command("nolimits") {
            self.pos += 9;
            under_over = false;
        }
        self.skip_ws();
        if matches!(self.peek(), Some('_' | '^')) {
            // `scripts` (called by our caller) takes the bounds and the body.
            return Some(Node::Nary {
                chr,
                under_over,
                sub: None,
                sup: None,
                body: Vec::new(),
            });
        }
        let body = self.nary_body();
        Some(Node::Nary {
            chr,
            under_over,
            sub: None,
            sup: None,
            body,
        })
    }

    /// `\left( … \middle| … \right)`.
    fn left_right(&mut self) -> Node {
        let beg = self.delimiter();
        let mut items = Vec::new();
        let mut sep = String::new();
        let end;
        loop {
            items.push(self.seq(Stop::Right));
            if self.at_command("middle") {
                self.pos += 7;
                sep = self.delimiter();
                continue;
            }
            if self.at_command("right") {
                self.pos += 6;
                end = self.delimiter();
            } else {
                self.warn("`\\left` without `\\right`");
                end = String::new();
            }
            break;
        }
        Node::Delim {
            beg,
            end,
            sep,
            items,
        }
    }

    /// `\begin{env} … \end{env}`.
    fn environment(&mut self) -> Node {
        let name = self.raw_argument();
        let name = name.trim();
        let (open, close, jcs): (&str, &str, Vec<&'static str>) = match name {
            "matrix" | "smallmatrix" => ("", "", Vec::new()),
            "pmatrix" => ("(", ")", Vec::new()),
            "bmatrix" => ("[", "]", Vec::new()),
            "Bmatrix" => ("{", "}", Vec::new()),
            "vmatrix" => ("|", "|", Vec::new()),
            "Vmatrix" => ("‖", "‖", Vec::new()),
            "cases" | "dcases" => ("{", "", vec!["left", "left"]),
            "rcases" => ("", "}", vec!["left", "left"]),
            "array" | "subarray" => {
                let spec = self.raw_argument();
                let jcs = spec
                    .chars()
                    .filter_map(|c| match c {
                        'l' => Some("left"),
                        'r' => Some("right"),
                        'c' => Some("center"),
                        _ => None,
                    })
                    .collect();
                ("", "", jcs)
            }
            "aligned" | "align" | "align*" | "split" | "alignat" | "alignat*" | "alignedat"
            | "eqnarray" | "eqnarray*" | "flalign" | "flalign*" => {
                if name.starts_with("alignat") || name == "alignedat" {
                    self.raw_argument();
                }
                ("", "", vec!["right", "left"])
            }
            "gathered" | "gather" | "gather*" | "multline" | "multline*" => {
                ("", "", vec!["center"])
            }
            "equation" | "equation*" | "displaymath" | "math" => {
                let body = self.seq(Stop::Cell);
                self.end_environment();
                return Node::Group(body);
            }
            other => {
                self.warn(&format!("the environment `{other}`"));
                ("", "", Vec::new())
            }
        };
        let rows = self.rows();
        self.end_environment();
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        // Column alignment: as declared, alternating right/left for align-like
        // environments, centred by default.
        let alternate = jcs == ["right", "left"];
        let jcs: Vec<&'static str> = (0..width)
            .map(|i| {
                if alternate {
                    if i % 2 == 0 {
                        "right"
                    } else {
                        "left"
                    }
                } else {
                    jcs.get(i)
                        .copied()
                        .or(jcs.last().copied())
                        .unwrap_or("center")
                }
            })
            .collect();
        let matrix = Node::Matrix { rows, jcs };
        if open.is_empty() && close.is_empty() {
            matrix
        } else {
            Node::Delim {
                beg: open.into(),
                end: close.into(),
                sep: String::new(),
                items: vec![vec![matrix]],
            }
        }
    }

    /// Rows of cells up to `\end`.
    fn rows(&mut self) -> Vec<Vec<Vec<Node>>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        loop {
            self.skip_ws();
            if self.at_command("hline") {
                self.pos += 6;
                continue;
            }
            let cell = self.seq(Stop::Cell);
            row.push(cell);
            if self.peek() == Some('&') {
                self.pos += 1;
                continue;
            }
            if self.at("\\\\") {
                self.pos += 2;
                self.skip_optional_length();
                rows.push(std::mem::take(&mut row));
                continue;
            }
            // \end, `}` or end of input.
            break;
        }
        // A trailing `\\` leaves an empty last row: drop it.
        if !(row.len() == 1 && row[0].is_empty()) {
            rows.push(row);
        }
        rows
    }

    fn end_environment(&mut self) {
        self.skip_ws();
        if self.at_command("end") {
            self.pos += 4;
            self.raw_argument();
        }
    }
}

/// Every run inside `nodes` takes `style` (`\mathbf{…}`), text runs excepted.
fn restyle(nodes: &mut [Node], style: RunStyle) {
    for node in nodes {
        match node {
            Node::Run(_, s) if !s.nor => *s = style,
            Node::Run(..) => {}
            Node::Group(inner) | Node::BorderBox(inner) => restyle(inner, style),
            Node::Frac { num, den, .. } => {
                restyle(num, style);
                restyle(den, style);
            }
            Node::Scripts { base, sub, sup } => {
                restyle(base, style);
                if let Some(s) = sub {
                    restyle(s, style);
                }
                if let Some(s) = sup {
                    restyle(s, style);
                }
            }
            Node::Rad { deg, body } => {
                if let Some(d) = deg {
                    restyle(d, style);
                }
                restyle(body, style);
            }
            Node::Nary { body, .. } => restyle(body, style),
            Node::Func { arg, .. } => restyle(arg, style),
            Node::Lim { base, .. } => restyle(base, style),
            Node::Delim { items, .. } => items.iter_mut().for_each(|i| restyle(i, style)),
            Node::Matrix { rows, .. } => rows.iter_mut().flatten().for_each(|c| restyle(c, style)),
            Node::Acc { body, .. } | Node::Bar { body, .. } | Node::GroupChr { body, .. } => {
                restyle(body, style);
            }
        }
    }
}

/// `\not=` → `≠`, `\not\in` → `∉`; any other symbol gets a combining long solidus.
fn negate(node: Node) -> Node {
    match node {
        Node::Run(text, style) => {
            let negated = match text.as_str() {
                "=" => "≠".to_string(),
                "∈" => "∉".to_string(),
                "<" => "≮".to_string(),
                ">" => "≯".to_string(),
                "≤" => "≰".to_string(),
                "≥" => "≱".to_string(),
                "⊂" => "⊄".to_string(),
                "⊃" => "⊅".to_string(),
                "⊆" => "⊈".to_string(),
                "⊇" => "⊉".to_string(),
                "≡" => "≢".to_string(),
                "∼" => "≁".to_string(),
                "≈" => "≉".to_string(),
                "∃" => "∄".to_string(),
                "|" | "∣" => "∤".to_string(),
                other => format!("{other}\u{338}"),
            };
            Node::Run(negated, style)
        }
        other => other,
    }
}

/// Functions with limits underneath in display style.
const LIMIT_FUNCTIONS: &[&str] = &[
    "lim", "liminf", "limsup", "max", "min", "sup", "inf", "argmax", "argmin", "det", "gcd", "Pr",
];

/// Upright function names.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "coth", "log", "ln", "lg", "exp", "dim", "ker", "deg", "hom", "arg", "mod", "bmod", "tr",
    "rank", "sgn", "diag", "erf", "Var", "Cov",
];

/// N-ary operators.
fn nary(name: &str) -> Option<char> {
    Some(match name {
        "sum" => '∑',
        "prod" => '∏',
        "coprod" => '∐',
        "int" => '∫',
        "iint" => '∬',
        "iiint" => '∭',
        "oint" => '∮',
        "oiint" => '∯',
        "bigcup" => '⋃',
        "bigcap" => '⋂',
        "bigoplus" => '⨁',
        "bigotimes" => '⨂',
        "bigodot" => '⨀',
        "biguplus" => '⨄',
        "bigsqcup" => '⨆',
        "bigvee" => '⋁',
        "bigwedge" => '⋀',
        _ => return None,
    })
}

/// Accent commands and the combining character OMML draws.
fn accent(name: &str) -> Option<char> {
    Some(match name {
        "hat" | "widehat" => '\u{302}',
        "bar" => '\u{203e}',
        "vec" | "overrightarrow" => '\u{20d7}',
        "overleftarrow" => '\u{20d6}',
        "tilde" | "widetilde" => '\u{303}',
        "dot" => '\u{307}',
        "ddot" => '\u{308}',
        "dddot" => '\u{20db}',
        "check" | "widecheck" => '\u{30c}',
        "breve" => '\u{306}',
        "acute" => '\u{301}',
        "grave" => '\u{300}',
        "mathring" => '\u{30a}',
        _ => return None,
    })
}

/// Font commands.
fn font(name: &str) -> Option<RunStyle> {
    let s = |sty: &'static str, scr: Option<&'static str>| RunStyle {
        sty: Some(sty),
        scr,
        nor: false,
    };
    Some(match name {
        "mathbf" => s("b", None),
        "mathrm" | "rm" | "mathup" => s("p", None),
        "mathit" | "it" => s("i", None),
        "boldsymbol" | "bm" | "mathbfit" | "pmb" => s("bi", None),
        "mathbb" | "Bbb" => s("p", Some("double-struck")),
        "mathcal" | "cal" => s("p", Some("script")),
        "mathscr" => s("p", Some("script")),
        "mathfrak" | "frak" => s("p", Some("fraktur")),
        "mathsf" | "sf" => s("p", Some("sans-serif")),
        "mathtt" | "tt" => s("p", Some("monospace")),
        _ => return None,
    })
}

/// Symbols: Greek letters (lower case italic, upper case upright, as in TeX), operators,
/// relations, arrows, dots, spaces and escaped characters.
fn symbol(name: &str) -> Option<Node> {
    let italic = |s: &str| Node::Run(s.to_string(), RunStyle::default());
    let upright = |s: &str| Node::Run(s.to_string(), UPRIGHT);
    let greek_lower = match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "varkappa" => "ϰ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "omicron" => "ο",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "ell" => "ℓ",
        "imath" => "ı",
        "jmath" => "ȷ",
        "hbar" => "ℏ",
        "wp" => "℘",
        _ => "",
    };
    if !greek_lower.is_empty() {
        return Some(italic(greek_lower));
    }
    let s = match name {
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // Binary operators.
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" | "cdotp" | "centerdot" => "⋅",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "∙",
        "oplus" => "⊕",
        "ominus" => "⊖",
        "otimes" => "⊗",
        "oslash" => "⊘",
        "odot" => "⊙",
        "cup" => "∪",
        "cap" => "∩",
        "sqcup" => "⊔",
        "sqcap" => "⊓",
        "uplus" => "⊎",
        "setminus" | "smallsetminus" => "∖",
        "wedge" | "land" => "∧",
        "vee" | "lor" => "∨",
        "neg" | "lnot" => "¬",
        "dagger" => "†",
        "ddagger" => "‡",
        "amalg" => "⨿",
        "wr" => "≀",
        // Relations.
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "leqslant" => "⩽",
        "geqslant" => "⩾",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" | "varpropto" => "∝",
        "in" => "∈",
        "notin" => "∉",
        "ni" | "owns" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "subsetneq" => "⊊",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "supsetneq" => "⊋",
        "ll" => "≪",
        "gg" => "≫",
        "prec" => "≺",
        "succ" => "≻",
        "preceq" => "⪯",
        "succeq" => "⪰",
        "perp" => "⊥",
        "parallel" => "∥",
        "mid" => "∣",
        "nmid" => "∤",
        "models" => "⊨",
        "vdash" => "⊢",
        "dashv" => "⊣",
        "doteq" => "≐",
        "asymp" => "≍",
        "triangleq" => "≜",
        "coloneqq" => "≔",
        // Arrows.
        "to" | "rightarrow" => "→",
        "gets" | "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "implies" | "Longrightarrow" => "⟹",
        "impliedby" | "Longleftarrow" => "⟸",
        "iff" | "Longleftrightarrow" => "⟺",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        "longleftrightarrow" => "⟷",
        "mapsto" => "↦",
        "longmapsto" => "⟼",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "updownarrow" => "↕",
        "Uparrow" => "⇑",
        "Downarrow" => "⇓",
        "nearrow" => "↗",
        "searrow" => "↘",
        "swarrow" => "↙",
        "nwarrow" => "↖",
        "hookrightarrow" => "↪",
        "hookleftarrow" => "↩",
        "leadsto" | "rightsquigarrow" => "⇝",
        "rightleftharpoons" => "⇌",
        "leftharpoonup" => "↼",
        "rightharpoonup" => "⇀",
        // Miscellaneous.
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "emptyset" | "varnothing" => "∅",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "beth" => "ℶ",
        "angle" => "∠",
        "measuredangle" => "∡",
        "triangle" | "vartriangle" => "△",
        "square" | "Box" => "□",
        "blacksquare" => "■",
        "diamond" => "⋄",
        "Diamond" => "◊",
        "top" => "⊤",
        "bot" => "⊥",
        "therefore" => "∴",
        "because" => "∵",
        "checkmark" => "✓",
        "surd" => "√",
        "degree" => "°",
        "prime" => "′",
        "flat" => "♭",
        "sharp" => "♯",
        "natural" => "♮",
        "clubsuit" => "♣",
        "diamondsuit" => "♢",
        "heartsuit" => "♡",
        "spadesuit" => "♠",
        "ldots" | "dots" | "dotsc" | "dotso" | "mathellipsis" => "…",
        "cdots" | "dotsb" | "dotsm" | "dotsi" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lbrace" | "{" => "{",
        "rbrace" | "}" => "}",
        "vert" | "lvert" | "rvert" => "|",
        "|" | "Vert" | "lVert" | "rVert" => "‖",
        "backslash" => "\\",
        "colon" => ":",
        "%" => "%",
        "#" => "#",
        "&" => "&",
        "$" => "$",
        "_" => "_",
        "/" => "/",
        // Spaces.
        "," | "thinspace" => "\u{2009}",
        ":" | ">" | "medspace" => "\u{205f}",
        ";" | "thickspace" => "\u{2004}",
        " " => "\u{a0}",
        "quad" => "\u{2003}",
        "qquad" => "\u{2003}\u{2003}",
        "enspace" => "\u{2002}",
        _ => return None,
    };
    Some(upright(s))
}

// ---- Writing OMML ----

fn esc(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn write_nodes(nodes: &[Node], out: &mut String) {
    for node in nodes {
        write_node(node, out);
    }
}

/// `<m:tag>…</m:tag>` around the nodes (`<m:tag/>` when empty).
fn wrap(tag: &str, nodes: &[Node], out: &mut String) {
    let mut inner = String::new();
    write_nodes(nodes, &mut inner);
    if inner.is_empty() {
        out.push_str(&format!("<m:{tag}/>"));
    } else {
        out.push_str(&format!("<m:{tag}>{inner}</m:{tag}>"));
    }
}

fn val(tag: &str, value: &str, out: &mut String) {
    out.push_str(&format!("<m:{tag} m:val=\""));
    esc(value, out);
    out.push_str("\"/>");
}

fn write_node(node: &Node, out: &mut String) {
    match node {
        Node::Run(text, style) => {
            if text.is_empty() {
                return;
            }
            out.push_str("<m:r>");
            if style.nor || style.scr.is_some() || style.sty.is_some() {
                out.push_str("<m:rPr>");
                if style.nor {
                    out.push_str("<m:nor/>");
                } else {
                    if let Some(scr) = style.scr {
                        val("scr", scr, out);
                    }
                    if let Some(sty) = style.sty {
                        val("sty", sty, out);
                    }
                }
                out.push_str("</m:rPr>");
            }
            if text.starts_with(' ') || text.ends_with(' ') {
                out.push_str("<m:t xml:space=\"preserve\">");
            } else {
                out.push_str("<m:t>");
            }
            esc(text, out);
            out.push_str("</m:t></m:r>");
        }
        Node::Group(inner) => write_nodes(inner, out),
        Node::Frac { num, den, bar } => {
            out.push_str("<m:f><m:fPr>");
            val("type", if *bar { "bar" } else { "noBar" }, out);
            out.push_str("</m:fPr>");
            wrap("num", num, out);
            wrap("den", den, out);
            out.push_str("</m:f>");
        }
        Node::Scripts { base, sub, sup } => {
            let tag = match (sub, sup) {
                (Some(_), Some(_)) => "sSubSup",
                (Some(_), None) => "sSub",
                _ => "sSup",
            };
            out.push_str(&format!("<m:{tag}>"));
            wrap("e", base, out);
            if let Some(sub) = sub {
                wrap("sub", sub, out);
            }
            if let Some(sup) = sup {
                wrap("sup", sup, out);
            }
            out.push_str(&format!("</m:{tag}>"));
        }
        Node::Rad { deg, body } => {
            out.push_str("<m:rad>");
            match deg {
                Some(deg) if !deg.is_empty() => wrap("deg", deg, out),
                _ => {
                    out.push_str("<m:radPr>");
                    val("degHide", "on", out);
                    out.push_str("</m:radPr><m:deg/>");
                }
            }
            wrap("e", body, out);
            out.push_str("</m:rad>");
        }
        Node::Nary {
            chr,
            under_over,
            sub,
            sup,
            body,
        } => {
            out.push_str("<m:nary><m:naryPr>");
            val("chr", &chr.to_string(), out);
            val("limLoc", if *under_over { "undOvr" } else { "subSup" }, out);
            val("subHide", if sub.is_some() { "off" } else { "on" }, out);
            val("supHide", if sup.is_some() { "off" } else { "on" }, out);
            out.push_str("</m:naryPr>");
            wrap("sub", sub.as_deref().unwrap_or(&[]), out);
            wrap("sup", sup.as_deref().unwrap_or(&[]), out);
            wrap("e", body, out);
            out.push_str("</m:nary>");
        }
        Node::Func { name, arg } => {
            if arg.is_empty() && !matches!(name.first(), Some(Node::Lim { .. })) {
                // A bare `\max`: just its name.
                write_nodes(name, out);
                return;
            }
            out.push_str("<m:func>");
            wrap("fName", name, out);
            wrap("e", arg, out);
            out.push_str("</m:func>");
        }
        Node::Lim { upper, base, lim } => {
            let tag = if *upper { "limUpp" } else { "limLow" };
            out.push_str(&format!("<m:{tag}>"));
            wrap("e", base, out);
            wrap("lim", lim, out);
            out.push_str(&format!("</m:{tag}>"));
        }
        Node::Delim {
            beg,
            end,
            sep,
            items,
        } => {
            out.push_str("<m:d><m:dPr>");
            val("begChr", beg, out);
            val("sepChr", sep, out);
            val("endChr", end, out);
            out.push_str("<m:grow/></m:dPr>");
            for item in items {
                wrap("e", item, out);
            }
            out.push_str("</m:d>");
        }
        Node::Matrix { rows, jcs } => {
            out.push_str("<m:m><m:mPr>");
            val("baseJc", "center", out);
            val("plcHide", "on", out);
            out.push_str("<m:mcs>");
            for jc in jcs {
                out.push_str("<m:mc><m:mcPr>");
                val("count", "1", out);
                val("mcJc", jc, out);
                out.push_str("</m:mcPr></m:mc>");
            }
            out.push_str("</m:mcs></m:mPr>");
            for row in rows {
                out.push_str("<m:mr>");
                for i in 0..jcs.len().max(1) {
                    wrap("e", row.get(i).map_or(&[][..], Vec::as_slice), out);
                }
                out.push_str("</m:mr>");
            }
            out.push_str("</m:m>");
        }
        Node::Acc { chr, body } => {
            out.push_str("<m:acc><m:accPr>");
            val("chr", &chr.to_string(), out);
            out.push_str("</m:accPr>");
            wrap("e", body, out);
            out.push_str("</m:acc>");
        }
        Node::Bar { top, body } => {
            out.push_str("<m:bar><m:barPr>");
            val("pos", if *top { "top" } else { "bot" }, out);
            out.push_str("</m:barPr>");
            wrap("e", body, out);
            out.push_str("</m:bar>");
        }
        Node::GroupChr { chr, top, body } => {
            out.push_str("<m:groupChr><m:groupChrPr>");
            val("chr", &chr.to_string(), out);
            val("pos", if *top { "top" } else { "bot" }, out);
            val("vertJc", if *top { "bot" } else { "top" }, out);
            out.push_str("</m:groupChrPr>");
            wrap("e", body, out);
            out.push_str("</m:groupChr>");
        }
        Node::BorderBox(body) => {
            out.push_str("<m:borderBox>");
            wrap("e", body, out);
            out.push_str("</m:borderBox>");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inside of `<m:oMath>` for inline `tex`.
    fn m(tex: &str) -> String {
        let out = convert(tex, false);
        assert!(out.warnings.is_empty(), "{tex}: {:?}", out.warnings);
        out.xml
            .strip_prefix("<m:oMath>")
            .and_then(|s| s.strip_suffix("</m:oMath>"))
            .unwrap()
            .to_string()
    }

    const P: &str = "<m:rPr><m:sty m:val=\"p\"/></m:rPr>";

    fn r(t: &str) -> String {
        format!("<m:r><m:t>{t}</m:t></m:r>")
    }

    fn rp(t: &str) -> String {
        format!("<m:r>{P}<m:t>{t}</m:t></m:r>")
    }

    #[test]
    fn letters_digits_and_operators() {
        assert_eq!(m("x"), r("x"));
        assert_eq!(m("12.5"), r("12.5"));
        assert_eq!(m("a+b"), format!("{}{}{}", r("a"), rp("+"), r("b")));
        assert_eq!(m("a - b"), format!("{}{}{}", r("a"), rp("−"), r("b")));
        assert_eq!(
            m("E=mc^2"),
            format!(
                "{}{}{}<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
                r("E"),
                rp("="),
                r("m"),
                r("c"),
                r("2")
            )
        );
    }

    #[test]
    fn fractions() {
        let frac = format!(
            "<m:f><m:fPr><m:type m:val=\"bar\"/></m:fPr><m:num>{}</m:num><m:den>{}</m:den></m:f>",
            r("a"),
            r("b")
        );
        assert_eq!(m(r"\frac{a}{b}"), frac);
        assert_eq!(m(r"\dfrac{a}{b}"), frac);
        assert_eq!(m(r"\tfrac ab"), frac);
        // `\frac12`: one digit per argument.
        assert_eq!(
            m(r"\frac12"),
            format!("<m:f><m:fPr><m:type m:val=\"bar\"/></m:fPr><m:num>{}</m:num><m:den>{}</m:den></m:f>", r("1"), r("2"))
        );
        assert!(m(r"\binom{n}{k}").contains("<m:type m:val=\"noBar\"/>"));
        assert!(m(r"\binom{n}{k}").starts_with("<m:d><m:dPr><m:begChr m:val=\"(\"/>"));
    }

    #[test]
    fn scripts() {
        assert_eq!(
            m("x_i^2"),
            format!(
                "<m:sSubSup><m:e>{}</m:e><m:sub>{}</m:sub><m:sup>{}</m:sup></m:sSubSup>",
                r("x"),
                r("i"),
                r("2")
            )
        );
        assert_eq!(m("x^2_i"), m("x_i^2"));
        assert_eq!(
            m("a_{ij}"),
            format!(
                "<m:sSub><m:e>{}</m:e><m:sub>{}{}</m:sub></m:sSub>",
                r("a"),
                r("i"),
                r("j")
            )
        );
        assert_eq!(
            m("f'"),
            format!(
                "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
                r("f"),
                rp("′")
            )
        );
        assert_eq!(
            m("e^{-x^2}"),
            format!(
                "<m:sSup><m:e>{}</m:e><m:sup>{}<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup></m:sup></m:sSup>",
                r("e"),
                rp("−"),
                r("x"),
                r("2")
            )
        );
    }

    #[test]
    fn roots() {
        assert_eq!(
            m(r"\sqrt{x}"),
            format!(
                "<m:rad><m:radPr><m:degHide m:val=\"on\"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>",
                r("x")
            )
        );
        assert_eq!(
            m(r"\sqrt[3]{x}"),
            format!(
                "<m:rad><m:deg>{}</m:deg><m:e>{}</m:e></m:rad>",
                r("3"),
                r("x")
            )
        );
    }

    #[test]
    fn nary_operators() {
        assert_eq!(
            m(r"\sum_{i=1}^{n} i"),
            format!(
                "<m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/><m:subHide m:val=\"off\"/><m:supHide m:val=\"off\"/></m:naryPr><m:sub>{}{}{}</m:sub><m:sup>{}</m:sup><m:e>{}</m:e></m:nary>",
                r("i"), rp("="), r("1"), r("n"), r("i")
            )
        );
        let int = m(r"\int_0^1 f\,dx");
        assert!(
            int.starts_with("<m:nary><m:naryPr><m:chr m:val=\"∫\"/><m:limLoc m:val=\"subSup\"/>"),
            "{int}"
        );
        assert!(
            int.contains(&format!("<m:e>{}</m:e></m:nary>", r("f"))),
            "{int}"
        );
        // No bounds: both hidden.
        let prod = m(r"\prod a_k");
        assert!(
            prod.contains("<m:subHide m:val=\"on\"/><m:supHide m:val=\"on\"/>"),
            "{prod}"
        );
        assert!(
            prod.contains("<m:e><m:sSub>"),
            "the body keeps its scripts: {prod}"
        );
        assert!(m(r"\oint F").contains("<m:chr m:val=\"∮\"/>"));
        assert!(m(r"\iint_D f").contains("<m:chr m:val=\"∬\"/>"));
        assert!(m(r"\bigcup_{i} A_i").contains("<m:chr m:val=\"⋃\"/><m:limLoc m:val=\"undOvr\"/>"));
    }

    #[test]
    fn limits_use_lim_low() {
        assert_eq!(
            m(r"\lim_{x \to 0} f"),
            format!(
                "<m:func><m:fName><m:limLow><m:e>{}</m:e><m:lim>{}{}{}</m:lim></m:limLow></m:fName><m:e>{}</m:e></m:func>",
                rp("lim"), r("x"), rp("→"), r("0"), r("f")
            )
        );
        assert!(m(r"\max_{x} g")
            .contains("<m:limLow><m:e><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>max</m:t>"));
        assert!(m(r"\operatorname*{argmax}_\theta L").contains("<m:limLow>"));
        // Without a subscript, `\max` is just its name; `\operatorname` takes scripts.
        assert_eq!(m(r"\max"), rp("max"));
        assert_eq!(
            m(r"\operatorname{argmax}_x f"),
            format!(
                "<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>{}",
                rp("argmax"),
                r("x"),
                r("f")
            )
        );
    }

    #[test]
    fn delimiters() {
        let d = |beg: &str, end: &str, inner: &str| {
            format!("<m:d><m:dPr><m:begChr m:val=\"{beg}\"/><m:sepChr m:val=\"\"/><m:endChr m:val=\"{end}\"/><m:grow/></m:dPr><m:e>{inner}</m:e></m:d>")
        };
        assert_eq!(m(r"\left( x \right)"), d("(", ")", &r("x")));
        assert_eq!(m(r"\left\{ x \right."), d("{", "", &r("x")));
        assert_eq!(m(r"\left\langle x \right\rangle"), d("⟨", "⟩", &r("x")));
        // Matched bare parentheses grow too (texmath does the same); unmatched stay text.
        assert_eq!(m("f(x)"), format!("{}{}", r("f"), d("(", ")", &r("x"))));
        assert_eq!(
            m("(0, 1]"),
            format!("{}{}{}{}{}", rp("("), r("0"), rp(","), r("1"), rp("]"))
        );
        let mid = m(r"\left\{ x \middle| x > 0 \right\}");
        assert!(mid.contains("<m:sepChr m:val=\"|\"/>"), "{mid}");
        assert_eq!(mid.matches("<m:e>").count(), 2, "{mid}");
        assert_eq!(
            m(r"\big( x \big)"),
            format!("{}{}{}", rp("("), r("x"), rp(")"))
        );
    }

    #[test]
    fn matrices() {
        let pm = m(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
        assert!(
            pm.starts_with("<m:d><m:dPr><m:begChr m:val=\"(\"/>"),
            "{pm}"
        );
        assert!(pm.contains("<m:m><m:mPr><m:baseJc m:val=\"center\"/><m:plcHide m:val=\"on\"/><m:mcs><m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"center\"/></m:mcPr></m:mc><m:mc>"), "{pm}");
        assert!(
            pm.contains(&format!(
                "<m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr><m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr>",
                r("a"),
                r("b"),
                r("c"),
                r("d")
            )),
            "{pm}"
        );
        assert!(m(r"\begin{bmatrix} 1 \\ 2 \end{bmatrix}").contains("<m:begChr m:val=\"[\"/>"));
        let plain = m(r"\begin{matrix} 1 & 0 \\ 0 & 1 \\ \end{matrix}");
        assert!(plain.starts_with("<m:m>"), "{plain}");
        assert_eq!(
            plain.matches("<m:mr>").count(),
            2,
            "a trailing \\\\ adds no row: {plain}"
        );
        let cases = m(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{else} \end{cases}");
        assert!(
            cases.contains("<m:begChr m:val=\"{\"/><m:sepChr m:val=\"\"/><m:endChr m:val=\"\"/>"),
            "{cases}"
        );
        assert!(cases.contains("<m:mcJc m:val=\"left\"/>"), "{cases}");
        assert!(
            cases.contains("<m:rPr><m:nor/></m:rPr><m:t>else</m:t>"),
            "{cases}"
        );
        let aligned = m(r"\begin{aligned} a &= b \\ c &= d \end{aligned}");
        assert!(aligned.contains("<m:mcJc m:val=\"right\"/></m:mcPr></m:mc><m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"left\"/>"), "{aligned}");
        let array = m(r"\begin{array}{lr} a & b \end{array}");
        assert!(
            array.contains("<m:mcJc m:val=\"left\"/>")
                && array.contains("<m:mcJc m:val=\"right\"/>"),
            "{array}"
        );
    }

    #[test]
    fn accents_and_bars() {
        let acc = |chr: &str, x: &str| {
            format!(
                "<m:acc><m:accPr><m:chr m:val=\"{chr}\"/></m:accPr><m:e>{}</m:e></m:acc>",
                r(x)
            )
        };
        assert_eq!(m(r"\hat{x}"), acc("\u{302}", "x"));
        assert_eq!(m(r"\bar{x}"), acc("\u{203e}", "x"));
        assert_eq!(m(r"\vec{v}"), acc("\u{20d7}", "v"));
        assert_eq!(m(r"\dot{x}"), acc("\u{307}", "x"));
        assert_eq!(m(r"\tilde x"), acc("\u{303}", "x"));
        assert_eq!(
            m(r"\overline{z}"),
            format!(
                "<m:bar><m:barPr><m:pos m:val=\"top\"/></m:barPr><m:e>{}</m:e></m:bar>",
                r("z")
            )
        );
        let brace = m(r"\underbrace{a+b}_{n}");
        assert!(brace.starts_with("<m:limLow><m:e><m:groupChr><m:groupChrPr><m:chr m:val=\"⏟\"/><m:pos m:val=\"bot\"/>"), "{brace}");
    }

    #[test]
    fn fonts() {
        assert_eq!(
            m(r"\mathbf{x}"),
            "<m:r><m:rPr><m:sty m:val=\"b\"/></m:rPr><m:t>x</m:t></m:r>"
        );
        assert_eq!(
            m(r"\mathbb{R}"),
            "<m:r><m:rPr><m:scr m:val=\"double-struck\"/><m:sty m:val=\"p\"/></m:rPr><m:t>R</m:t></m:r>"
        );
        assert_eq!(
            m(r"\mathcal{L}"),
            "<m:r><m:rPr><m:scr m:val=\"script\"/><m:sty m:val=\"p\"/></m:rPr><m:t>L</m:t></m:r>"
        );
        assert_eq!(m(r"\mathrm{d}x"), format!("{}{}", rp("d"), r("x")));
        assert_eq!(
            m(r"\text{if } x"),
            format!(
                "<m:r><m:rPr><m:nor/></m:rPr><m:t xml:space=\"preserve\">if </m:t></m:r>{}",
                r("x")
            )
        );
        assert_eq!(m(r"\operatorname{tr} A"), format!("{}{}", rp("tr"), r("A")));
    }

    #[test]
    fn greek_and_symbols() {
        assert_eq!(
            m(r"\alpha \leq \beta"),
            format!("{}{}{}", r("α"), rp("≤"), r("β"))
        );
        assert_eq!(m(r"\Omega"), rp("Ω"));
        assert_eq!(m(r"x \in \mathbb{N}"), format!("{}{}<m:r><m:rPr><m:scr m:val=\"double-struck\"/><m:sty m:val=\"p\"/></m:rPr><m:t>N</m:t></m:r>", r("x"), rp("∈")));
        assert_eq!(m(r"a \not= b"), format!("{}{}{}", r("a"), rp("≠"), r("b")));
        assert_eq!(m(r"\infty"), rp("∞"));
        assert_eq!(m(r"a<b"), format!("{}{}{}", r("a"), rp("&lt;"), r("b")));
        assert_eq!(m(r"\{x\}"), format!("{}{}{}", rp("{"), r("x"), rp("}")));
        assert_eq!(m(r"\sin x"), format!("{}{}", rp("sin"), r("x")));
    }

    #[test]
    fn display_math_is_a_math_paragraph() {
        let out = convert(r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", true);
        assert!(out.warnings.is_empty());
        assert!(out.xml.starts_with(
            "<m:oMathPara><m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr><m:oMath><m:f>"
        ));
        assert!(out.xml.ends_with("</m:f></m:oMath></m:oMathPara>"));
        // The same structure Pandoc writes for the quadratic formula (math.md fixture).
        assert!(out
            .xml
            .contains(&format!("<m:num>{}{}{}<m:rad>", rp("−"), r("b"), rp("±"))));
        assert!(out
            .xml
            .contains(&format!("<m:den>{}{}</m:den>", r("2"), r("a"))));
    }

    #[test]
    fn top_level_line_breaks_become_an_equation_array() {
        let out = convert(r"a = b \\ c = d", true);
        assert!(out.xml.contains("<m:eqArr><m:e>"), "{}", out.xml);
        assert_eq!(out.xml.matches("<m:e>").count(), 2);
    }

    #[test]
    fn unknown_commands_degrade_to_text_with_a_warning() {
        let out = convert(r"\frac{a}{\weird{b}} + \foo", false);
        assert_eq!(out.warnings.len(), 2, "{:?}", out.warnings);
        assert!(out.warnings.iter().all(|w| w.code == "math-unsupported"));
        assert!(out.xml.contains(r"<m:t>\weird</m:t>"));
        assert!(out.xml.contains(r"<m:t>\foo</m:t>"));
        // Still a fraction around the unknown part.
        assert!(out.xml.starts_with("<m:oMath><m:f>"));
    }

    #[test]
    fn garbage_never_panics() {
        for tex in [
            "",
            "}",
            "{",
            "^",
            "_",
            "x^",
            r"\frac",
            r"\left(",
            r"\right)",
            r"\begin{pmatrix}",
            r"\end{x}",
            "&&",
            r"\\\\",
            r"\sqrt[",
            "((((",
            r"\sum_",
            "'''",
            r"\not",
            r"\text{",
            r"\mathbf",
            "a_b_c",
            "x^{y^{z^{w}}}",
        ] {
            let out = convert(tex, true);
            assert!(out.xml.starts_with("<m:oMathPara>"), "{tex}");
        }
    }

    #[test]
    fn strict_entry_point_rejects_unbalanced_braces() {
        assert!(latex_to_omml("{", false).is_err());
        assert!(latex_to_omml("}", false).is_err());
        assert!(latex_to_omml(r"\{ x \}", false).is_ok());
        assert_eq!(
            latex_to_omml("x", false).unwrap(),
            format!("<m:oMath>{}</m:oMath>", r("x"))
        );
    }

    #[test]
    fn output_is_well_formed_xml() {
        for tex in [
            r"\sum_{i=1}^n x_i^2 + \int_0^\infty e^{-t}\,dt",
            r"\left[ \begin{array}{cc} 1 & 2 \\ 3 & 4 \end{array} \right]",
            r"\overbrace{1+\cdots+1}^{n} \le \boxed{x} \xrightarrow{f} \overset{!}{=} y",
            r#"a < b > c & d "e""#,
        ] {
            let xml = convert(tex, true).xml;
            let wrapped = format!("<root xmlns:m=\"urn:m\">{xml}</root>");
            let mut reader = quick_xml::Reader::from_str(&wrapped);
            loop {
                match reader.read_event() {
                    Ok(quick_xml::events::Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{tex}: {e}\n{xml}"),
                }
            }
        }
    }
}
