//! LaTeX math → Typst math, for the subset md2star documents use: fractions, scripts,
//! roots, big operators with bounds, limits, delimiters (`\left…\right` and plain),
//! matrices and `cases`, aligned equations, accents, over/under braces, font commands
//! (`\mathbf`, `\mathbb`, …), `\text`, `\operatorname`, Greek letters, operators,
//! relations and arrows.
//!
//! Symbols are written as their Unicode characters rather than Typst's symbol names:
//! Typst sets a character exactly as it sets the named symbol, and characters do not
//! drift when Typst renames a symbol. Every token is separated by a space, so two
//! letters never fuse into one (unknown) Typst identifier and `<` `=` never fuse into
//! a `<=` shorthand. A command outside the subset is kept visible as its own name, in
//! quotes, and reported.

/// The Typst math source for `tex`, and the commands that were not understood.
pub fn to_typst(tex: &str) -> (String, Vec<String>) {
    let tokens = tokenize(tex);
    let mut cx = Cx {
        tokens,
        pos: 0,
        unknown: Vec::new(),
    };
    let items = cx.sequence(&[]);
    let out = emit(&items, &mut cx.unknown, false);
    (tidy(&out), cx.unknown)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Cmd(String),
    Char(char),
    Open,
    Close,
    Sub,
    Sup,
    Amp,
    Row,
}

fn tokenize(tex: &str) -> Vec<Tok> {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                i += 1;
                if i >= chars.len() {
                    break;
                }
                if chars[i].is_ascii_alphabetic() {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let mut name: String = chars[start..i].iter().collect();
                    if i < chars.len() && chars[i] == '*' {
                        name.push('*');
                        i += 1;
                    }
                    out.push(Tok::Cmd(name));
                    continue;
                }
                if chars[i] == '\\' {
                    out.push(Tok::Row);
                } else {
                    out.push(Tok::Cmd(chars[i].to_string()));
                }
            }
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '_' => out.push(Tok::Sub),
            '^' => out.push(Tok::Sup),
            '&' => out.push(Tok::Amp),
            '%' => {
                // A comment runs to the end of the line.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            c if c.is_whitespace() => {}
            c => out.push(Tok::Char(c)),
        }
        i += 1;
    }
    out
}

/// The parsed math, close to Typst's own structure.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    /// Already Typst: a symbol, a number, an operator.
    Atom(String),
    /// A letter (a variable).
    Letter(char),
    /// A braced group.
    Group(Vec<Item>),
    /// `base_sub^sup`.
    Scripts {
        base: Box<Item>,
        sub: Option<Box<Item>>,
        sup: Option<Box<Item>>,
    },
    /// A function call: `name(arg, arg)` with optional named arguments first.
    Call {
        name: String,
        named: Vec<(String, String)>,
        args: Vec<Item>,
    },
    /// `lr(open … close)`.
    Lr(String, Vec<Item>, String),
    /// Rows of cells (matrices, cases, aligned): `named` + the grid.
    Grid {
        func: Option<(String, Vec<(String, String)>)>,
        rows: Vec<Vec<Vec<Item>>>,
    },
    /// Quoted text.
    Text(String),
    /// `&` alignment point (outside a grid function).
    AlignPoint,
    /// `\\` line break (outside a grid function).
    LineBreak,
}

struct Cx {
    tokens: Vec<Tok>,
    pos: usize,
    unknown: Vec<String>,
}

impl Cx {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// Items until a closing brace, `\right`, `\end` or one of `stop`.
    fn sequence(&mut self, stop: &[&str]) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        while let Some(tok) = self.peek().cloned() {
            match &tok {
                Tok::Close => break,
                Tok::Cmd(c) if c == "right" || c == "end" || stop.contains(&c.as_str()) => break,
                Tok::Sub | Tok::Sup => {
                    self.pos += 1;
                    let arg = self.argument();
                    let base = items.pop().unwrap_or(Item::Atom(String::new()));
                    let (base, mut sub, mut sup) = match base {
                        Item::Scripts { base, sub, sup } => (*base, sub, sup),
                        other => (other, None, None),
                    };
                    if tok == Tok::Sub {
                        sub = Some(Box::new(arg));
                    } else {
                        sup = Some(Box::new(arg));
                    }
                    items.push(Item::Scripts {
                        base: Box::new(base),
                        sub,
                        sup,
                    });
                }
                _ => {
                    if let Some(item) = self.atom() {
                        items.push(item);
                    }
                }
            }
        }
        items
    }

    /// One argument: a braced group or a single token.
    fn argument(&mut self) -> Item {
        match self.peek() {
            Some(Tok::Open) => {
                self.pos += 1;
                let inner = self.sequence(&[]);
                if self.peek() == Some(&Tok::Close) {
                    self.pos += 1;
                }
                Item::Group(inner)
            }
            Some(_) => self.atom().unwrap_or(Item::Group(Vec::new())),
            None => Item::Group(Vec::new()),
        }
    }

    /// Raw text of a braced argument (for `\text`, `\operatorname`, environment names).
    fn raw_argument(&mut self) -> String {
        let mut s = String::new();
        if self.peek() != Some(&Tok::Open) {
            return s;
        }
        self.pos += 1;
        let mut depth = 0;
        while let Some(t) = self.next() {
            match t {
                Tok::Open => {
                    depth += 1;
                    s.push('{');
                }
                Tok::Close if depth == 0 => break,
                Tok::Close => {
                    depth -= 1;
                    s.push('}');
                }
                Tok::Char(c) => s.push(c),
                Tok::Cmd(c) if c == " " => s.push(' '),
                Tok::Cmd(c) if c.len() == 1 => s.push_str(&c),
                Tok::Cmd(c) => {
                    s.push('\\');
                    s.push_str(&c);
                }
                Tok::Sub => s.push('_'),
                Tok::Sup => s.push('^'),
                Tok::Amp => s.push('&'),
                Tok::Row => s.push(' '),
            }
        }
        s
    }

    /// An optional `[…]` argument.
    fn optional(&mut self) -> Option<Vec<Item>> {
        if self.peek() != Some(&Tok::Char('[')) {
            return None;
        }
        self.pos += 1;
        let mut items = Vec::new();
        while let Some(t) = self.peek() {
            if *t == Tok::Char(']') {
                self.pos += 1;
                break;
            }
            match self.atom() {
                Some(i) => items.push(i),
                None => break,
            }
        }
        Some(items)
    }

    fn atom(&mut self) -> Option<Item> {
        let tok = self.next()?;
        Some(match tok {
            Tok::Open => {
                let inner = self.sequence(&[]);
                if self.peek() == Some(&Tok::Close) {
                    self.pos += 1;
                }
                Item::Group(inner)
            }
            Tok::Close => return None,
            Tok::Sub | Tok::Sup => return None,
            Tok::Amp => Item::AlignPoint,
            Tok::Row => Item::LineBreak,
            Tok::Char(c) => char_item(c),
            Tok::Cmd(name) => self.command(&name),
        })
    }

    fn command(&mut self, name: &str) -> Item {
        let call1 = |cx: &mut Cx, f: &str| Item::Call {
            name: f.into(),
            named: Vec::new(),
            args: vec![cx.argument()],
        };
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => Item::Call {
                name: "frac".into(),
                named: Vec::new(),
                args: vec![self.argument(), self.argument()],
            },
            "binom" | "dbinom" | "tbinom" => Item::Call {
                name: "binom".into(),
                named: Vec::new(),
                args: vec![self.argument(), self.argument()],
            },
            "sqrt" => match self.optional() {
                Some(index) => Item::Call {
                    name: "root".into(),
                    named: Vec::new(),
                    args: vec![Item::Group(index), self.argument()],
                },
                None => call1(self, "sqrt"),
            },
            "hat" | "widehat" => call1(self, "hat"),
            "tilde" | "widetilde" => call1(self, "tilde"),
            "bar" => call1(self, "macron"),
            "vec" => call1(self, "arrow"),
            "dot" => call1(self, "dot"),
            "ddot" => call1(self, "dot.double"),
            "check" => call1(self, "caron"),
            "breve" => call1(self, "breve"),
            "acute" => call1(self, "acute"),
            "grave" => call1(self, "grave"),
            "overline" => call1(self, "overline"),
            "underline" => call1(self, "underline"),
            "overbrace" | "underbrace" => {
                let body = self.argument();
                let mut args = vec![body];
                let script = if name == "overbrace" {
                    Tok::Sup
                } else {
                    Tok::Sub
                };
                if self.peek() == Some(&script) {
                    self.pos += 1;
                    args.push(self.argument());
                }
                Item::Call {
                    name: name.into(),
                    named: Vec::new(),
                    args,
                }
            }
            "mathbf" | "boldsymbol" | "bm" => call1(self, "bold"),
            "mathit" => call1(self, "italic"),
            "mathrm" => call1(self, "upright"),
            "mathbb" => call1(self, "bb"),
            "mathcal" | "mathscr" => call1(self, "cal"),
            "mathfrak" => call1(self, "frak"),
            "mathsf" => call1(self, "sans"),
            "mathtt" => call1(self, "mono"),
            "text" | "textrm" | "textnormal" | "mbox" | "textit" | "textbf" | "texttt" => {
                Item::Text(self.raw_argument())
            }
            "operatorname" | "operatorname*" => {
                let text = self.raw_argument();
                Item::Call {
                    name: "op".into(),
                    named: if name.ends_with('*') {
                        vec![("limits".into(), "#true".into())]
                    } else {
                        Vec::new()
                    },
                    args: vec![Item::Text(text)],
                }
            }
            "overset" | "stackrel" | "underset" => {
                let over = self.argument();
                let base = self.argument();
                let key = if name == "underset" { "b" } else { "t" };
                Item::Call {
                    name: "attach".into(),
                    named: Vec::new(),
                    args: vec![base, Item::Atom(format!("{key}: {}", emit_arg(&over)))],
                }
            }
            "xrightarrow" | "xleftarrow" => {
                let _ = self.optional();
                let label = self.argument();
                let arrow = if name == "xrightarrow" { "⟶" } else { "⟵" };
                Item::Call {
                    name: "attach".into(),
                    named: Vec::new(),
                    args: vec![
                        Item::Atom(arrow.into()),
                        Item::Atom(format!("t: {}", emit_arg(&label))),
                    ],
                }
            }
            "boxed" => self.argument(),
            "left" => {
                let open = self.delimiter();
                let inner = self.sequence(&["middle"]);
                let mut inner = inner;
                while self.peek() == Some(&Tok::Cmd("middle".into())) {
                    self.pos += 1;
                    let d = self.delimiter();
                    inner.push(Item::Atom(if d.is_empty() {
                        String::new()
                    } else {
                        format!("mid({d})")
                    }));
                    inner.extend(self.sequence(&["middle"]));
                }
                let close = if self.peek() == Some(&Tok::Cmd("right".into())) {
                    self.pos += 1;
                    self.delimiter()
                } else {
                    String::new()
                };
                Item::Lr(open, inner, close)
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" => Item::Atom(self.delimiter()),
            "begin" => self.environment(),
            "limits" | "nolimits" | "displaystyle" | "textstyle" | "scriptstyle" | "nonumber"
            | "notag" | "label" => {
                if name == "label" {
                    let _ = self.raw_argument();
                }
                Item::Atom(String::new())
            }
            "not" => {
                // `\not=` → ≠, `\not\in` → ∉; otherwise a slash over the next atom.
                match self.atom() {
                    Some(Item::Atom(a)) if a == "=" => Item::Atom("≠".into()),
                    Some(Item::Atom(a)) if a == "∈" => Item::Atom("∉".into()),
                    Some(other) => other,
                    None => Item::Atom(String::new()),
                }
            }
            other => match symbol(other) {
                Some(s) => Item::Atom(s.to_string()),
                None => match FUNCTIONS.contains(&other) {
                    true => Item::Atom(other.to_string()),
                    false => {
                        self.unknown.push(format!("\\{other}"));
                        Item::Text(format!("\\{other}"))
                    }
                },
            },
        }
    }

    /// The delimiter after `\left` / `\right` / `\big`, as Typst writes it.
    fn delimiter(&mut self) -> String {
        match self.next() {
            Some(Tok::Char('.')) | None => String::new(),
            Some(Tok::Char(c)) => delim_char(c),
            Some(Tok::Cmd(c)) => match c.as_str() {
                "{" | "lbrace" => "\\{".into(),
                "}" | "rbrace" => "\\}".into(),
                "|" | "Vert" => "‖".into(),
                "vert" | "lvert" | "rvert" | "mid" => "|".into(),
                "lVert" | "rVert" => "‖".into(),
                "langle" => "⟨".into(),
                "rangle" => "⟩".into(),
                "lfloor" => "⌊".into(),
                "rfloor" => "⌋".into(),
                "lceil" => "⌈".into(),
                "rceil" => "⌉".into(),
                "backslash" => "\\\\".into(),
                _ => String::new(),
            },
            Some(_) => String::new(),
        }
    }

    fn environment(&mut self) -> Item {
        let env = self.raw_argument();
        let base = env.trim_end_matches('*').to_string();
        // `array` takes a column spec; skip it.
        if base == "array" || base == "alignat" {
            let _ = self.raw_argument();
        }
        let mut rows: Vec<Vec<Vec<Item>>> = vec![vec![Vec::new()]];
        loop {
            match self.peek().cloned() {
                None => break,
                Some(Tok::Cmd(c)) if c == "end" => {
                    self.pos += 1;
                    let _ = self.raw_argument();
                    break;
                }
                Some(Tok::Amp) => {
                    self.pos += 1;
                    if let Some(row) = rows.last_mut() {
                        row.push(Vec::new());
                    }
                }
                Some(Tok::Row) => {
                    self.pos += 1;
                    rows.push(vec![Vec::new()]);
                }
                Some(Tok::Close) => {
                    self.pos += 1;
                }
                _ => {
                    let items = self.sequence_in_cell();
                    if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                        cell.extend(items);
                    }
                }
            }
        }
        // A trailing `\\` leaves an empty last row.
        if rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(Vec::is_empty)) {
            rows.pop();
        }
        let delim = |d: &str| vec![("delim".to_string(), format!("\"{d}\""))];
        let func = match base.as_str() {
            "matrix" | "smallmatrix" | "array" => Some((
                "mat".to_string(),
                vec![("delim".to_string(), "#none".to_string())],
            )),
            "pmatrix" => Some(("mat".to_string(), delim("("))),
            "bmatrix" => Some(("mat".to_string(), delim("["))),
            "Bmatrix" => Some(("mat".to_string(), delim("{"))),
            "vmatrix" => Some(("mat".to_string(), delim("|"))),
            "Vmatrix" => Some(("mat".to_string(), delim("‖"))),
            "cases" => Some(("cases".to_string(), Vec::new())),
            "aligned" | "align" | "split" | "gathered" | "gather" | "eqnarray" | "alignat"
            | "equation" | "multline" => None,
            other => {
                self.unknown.push(format!("environment {other}"));
                None
            }
        };
        Item::Grid { func, rows }
    }

    /// Items of one cell: stop at `&`, `\\` and `\end`.
    fn sequence_in_cell(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        while let Some(t) = self.peek().cloned() {
            match t {
                Tok::Amp | Tok::Row | Tok::Close => break,
                Tok::Cmd(ref c) if c == "end" => break,
                _ => {
                    let mut seq = self.sequence_until_cell_end();
                    if seq.is_empty() {
                        break;
                    }
                    items.append(&mut seq);
                }
            }
        }
        items
    }

    fn sequence_until_cell_end(&mut self) -> Vec<Item> {
        // `sequence` stops at `}` / `\right` / `\end`; cells also stop at `&` and `\\`,
        // which `atom` turns into AlignPoint / LineBreak — split them back out.
        let start = self.pos;
        let mut items = Vec::new();
        while let Some(t) = self.peek().cloned() {
            if matches!(t, Tok::Amp | Tok::Row | Tok::Close) {
                break;
            }
            if let Tok::Cmd(ref c) = t {
                if c == "end" {
                    break;
                }
            }
            match t {
                Tok::Sub | Tok::Sup => {
                    self.pos += 1;
                    let arg = self.argument();
                    let base = items.pop().unwrap_or(Item::Atom(String::new()));
                    let (base, mut sub, mut sup) = match base {
                        Item::Scripts { base, sub, sup } => (*base, sub, sup),
                        other => (other, None, None),
                    };
                    if t == Tok::Sub {
                        sub = Some(Box::new(arg));
                    } else {
                        sup = Some(Box::new(arg));
                    }
                    items.push(Item::Scripts {
                        base: Box::new(base),
                        sub,
                        sup,
                    });
                }
                _ => match self.atom() {
                    Some(i) => items.push(i),
                    None => break,
                },
            }
        }
        if self.pos == start {
            self.pos += 1;
        }
        items
    }
}

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "coth", "log", "ln", "lg", "exp", "lim", "max", "min", "sup", "inf", "det", "dim", "ker",
    "deg", "gcd", "arg", "Pr", "hom", "limsup", "liminf", "mod",
];

fn char_item(c: char) -> Item {
    match c {
        'a'..='z' | 'A'..='Z' => Item::Letter(c),
        '(' | ')' | '[' | ']' => Item::Atom(format!("\\{c}")),
        '/' => Item::Atom("\\/".into()),
        '\'' => Item::Atom("'".into()),
        ',' => Item::Atom(",".into()),
        ';' => Item::Atom("\\;".into()),
        '"' => Item::Atom("\\\"".into()),
        '#' => Item::Atom("\\#".into()),
        '$' => Item::Atom("\\$".into()),
        '~' => Item::Atom(" ".into()),
        '|' => Item::Atom("|".into()),
        '<' => Item::Atom("<".into()),
        '>' => Item::Atom(">".into()),
        '-' => Item::Atom("-".into()),
        '*' => Item::Atom("*".into()),
        '!' => Item::Atom("!".into()),
        '.' => Item::Atom(".".into()),
        ':' => Item::Atom(":".into()),
        '@' => Item::Atom("\\@".into()),
        c => Item::Atom(c.to_string()),
    }
}

fn delim_char(c: char) -> String {
    match c {
        '(' | ')' | '[' | ']' => c.to_string(),
        '|' => "|".into(),
        '<' => "⟨".into(),
        '>' => "⟩".into(),
        '/' => "\\/".into(),
        other => other.to_string(),
    }
}

/// A LaTeX command without arguments → the character Typst sets for it.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        // Greek, lower case.
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
        // Greek, upper case.
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
        // Big operators: Typst's names, so their limits go above and below in display.
        "sum" => "sum",
        "prod" => "product",
        "coprod" => "product.co",
        "int" => "integral",
        "iint" => "integral.double",
        "iiint" => "integral.triple",
        "oint" => "integral.cont",
        "bigcup" => "union.big",
        "bigcap" => "inter.big",
        "bigoplus" => "plus.circle.big",
        "bigotimes" => "times.circle.big",
        "bigvee" => "or.big",
        "bigwedge" => "and.big",
        // Binary operators.
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" => "⋅",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "∙",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "cup" => "∪",
        "cap" => "∩",
        "setminus" => "∖",
        "wedge" | "land" => "∧",
        "vee" | "lor" => "∨",
        // Relations.
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "ll" => "≪",
        "gg" => "≫",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "perp" => "⊥",
        "parallel" => "∥",
        "mid" => "|",
        "models" => "⊨",
        "vdash" => "⊢",
        // Arrows.
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        "Longrightarrow" => "⟹",
        "uparrow" => "↑",
        "downarrow" => "↓",
        // Miscellaneous.
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "emptyset" | "varnothing" => "∅",
        "ell" => "ℓ",
        "hbar" => "ℏ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "angle" => "∠",
        "prime" => "′",
        "degree" => "°",
        "ldots" | "dots" | "dotsc" => "…",
        "cdots" | "dotsb" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lvert" | "rvert" | "vert" => "|",
        "|" | "Vert" | "lVert" | "rVert" => "‖",
        "{" | "lbrace" => "\\{",
        "}" | "rbrace" => "\\}",
        "%" => "%",
        "&" => "&",
        "#" => "\\#",
        "$" => "\\$",
        "_" => "\\_",
        "backslash" => "\\\\",
        // Spaces.
        "," | "thinspace" => "thin",
        ":" | ">" | "medspace" => "med",
        ";" | "thickspace" => "thick",
        "!" => "",
        " " => " ",
        "quad" => "quad",
        "qquad" => "wide",
        _ => return None,
    })
}

fn emit(items: &[Item], unknown: &mut Vec<String>, in_args: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in items {
        parts.push(emit_item(item, unknown, in_args));
    }
    parts.join(" ")
}

fn emit_item(item: &Item, unknown: &mut Vec<String>, in_args: bool) -> String {
    match item {
        Item::Atom(a) => {
            if in_args && a == "," {
                "\\,".into()
            } else {
                a.clone()
            }
        }
        Item::Letter(c) => c.to_string(),
        // LaTeX braces group without showing; Typst parentheses would show.
        Item::Group(inner) => emit(inner, unknown, in_args),
        Item::Scripts { base, sub, sup } => {
            let mut s = emit_item(base, unknown, in_args);
            if s.is_empty() {
                s = "\"\"".into();
            }
            if let Some(b) = sub {
                s.push_str(&format!("_({})", emit_script(b, unknown)));
            }
            if let Some(p) = sup {
                let inner = emit_script(p, unknown);
                if !inner.is_empty() && inner.chars().all(|c| c == '\'' || c == '′') {
                    s.push_str(&"'".repeat(inner.chars().count()));
                } else {
                    s.push_str(&format!("^({inner})"));
                }
            }
            s
        }
        Item::Call { name, named, args } => {
            let mut all: Vec<String> = named.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            for a in args {
                match a {
                    // `attach` keys are already `t: …`.
                    Item::Atom(s)
                        if name == "attach" && (s.starts_with("t: ") || s.starts_with("b: ")) =>
                    {
                        all.push(s.clone())
                    }
                    other => all.push(emit_arg_with(other, unknown)),
                }
            }
            format!("{name}({})", all.join(", "))
        }
        Item::Lr(open, inner, close) => {
            let body = emit(inner, unknown, in_args);
            if open.is_empty() && close.is_empty() {
                body
            } else {
                format!("lr({open} {body} {close})")
            }
        }
        Item::Grid { func, rows } => match func {
            Some((name, named)) => {
                let mut all: Vec<String> = named.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                let body = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| emit(cell, unknown, true))
                            .map(|c| {
                                if c.trim().is_empty() {
                                    "\"\"".to_string()
                                } else {
                                    c
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(if name == "cases" { " & " } else { ", " })
                    })
                    .collect::<Vec<_>>()
                    .join(if name == "cases" { ", " } else { "; " });
                all.push(body);
                format!("{name}({})", all.join(", "))
            }
            None => rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| emit(cell, unknown, in_args))
                        .collect::<Vec<_>>()
                        .join(" & ")
                })
                .collect::<Vec<_>>()
                .join(" \\\n"),
        },
        Item::Text(t) => format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\"")),
        Item::AlignPoint => "&".into(),
        Item::LineBreak => "\\\n".into(),
    }
}

fn emit_script(item: &Item, unknown: &mut Vec<String>) -> String {
    match item {
        Item::Group(inner) => emit(inner, unknown, false),
        other => emit_item(other, unknown, false),
    }
}

fn emit_arg_with(item: &Item, unknown: &mut Vec<String>) -> String {
    let s = match item {
        Item::Group(inner) => emit(inner, unknown, true),
        other => emit_item(other, unknown, true),
    };
    if s.trim().is_empty() {
        "\"\"".into()
    } else {
        s
    }
}

fn emit_arg(item: &Item) -> String {
    let mut ignored = Vec::new();
    emit_arg_with(item, &mut ignored)
}

/// Collapse runs of spaces (newlines kept) and trim.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.trim().chars() {
        if c == ' ' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_typst;

    fn t(tex: &str) -> String {
        let (out, unknown) = to_typst(tex);
        assert!(unknown.is_empty(), "{tex}: {unknown:?}");
        out
    }

    #[test]
    fn letters_never_fuse_and_scripts_group() {
        assert_eq!(t("E = mc^2"), "E = m c^(2)");
        assert_eq!(t("x_{ij}^{n+1}"), "x_(i j)^(n + 1)");
        assert_eq!(t("f'(x)"), "f ' \\( x \\)");
    }

    #[test]
    fn fractions_roots_and_operators() {
        assert_eq!(t("\\frac{a+b}{c}"), "frac(a + b, c)");
        assert_eq!(t("\\sqrt[3]{x}"), "root(3, x)");
        assert_eq!(t("\\sum_{i=1}^{n} i"), "sum_(i = 1)^(n) i");
        assert_eq!(t("\\alpha \\leq \\beta"), "α ≤ β");
    }

    #[test]
    fn delimiters_and_matrices() {
        assert_eq!(t("\\left( x \\right)"), "lr(( x ))");
        assert_eq!(t("\\left\\{ x \\right."), "lr(\\{ x )");
        assert_eq!(
            t("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}"),
            "mat(delim: \"(\", a, b; c, d)"
        );
        assert_eq!(
            t("f(x) = \\begin{cases} 1 & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}"),
            "f \\( x \\) = cases(1 & x > 0, 0 & \"otherwise\")"
        );
    }

    #[test]
    fn commas_inside_arguments_are_escaped() {
        assert_eq!(t("\\frac{a, b}{c}"), "frac(a \\, b, c)");
    }

    #[test]
    fn unknown_commands_stay_visible_and_are_reported() {
        let (out, unknown) = to_typst("\\foo x");
        assert_eq!(out, "\"\\\\foo\" x");
        assert_eq!(unknown, vec!["\\foo".to_string()]);
    }
}
