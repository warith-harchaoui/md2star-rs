//! OMML (Office Math) → LaTeX, for the common subset: fractions, scripts, radicals,
//! n-ary operators, delimiters, matrices, accents, bars, functions, limits, group
//! characters, equation arrays, Greek letters and the usual operators.
//!
//! This is the inverse of the DOCX writer's LaTeX → OMML. A construct outside the
//! subset is not dropped: its text content is kept and the caller gets a warning, so
//! the twin says what it could not translate instead of silently inventing LaTeX.

use super::xml::Node;

/// The LaTeX of one `m:oMath` (or any OMML container), plus the local names of the
/// elements that were not understood.
pub fn to_latex(node: &Node) -> (String, Vec<String>) {
    let mut cx = Cx::default();
    let tex = cx.children(node);
    (tidy(&tex), cx.unknown)
}

#[derive(Default)]
struct Cx {
    unknown: Vec<String>,
}

impl Cx {
    fn children(&mut self, node: &Node) -> String {
        let mut out = String::new();
        for child in node.elems() {
            let piece = self.elem(child);
            push(&mut out, &piece);
        }
        out
    }

    /// The argument `name` of `node` (`m:e`, `m:num`…), braced.
    fn arg(&mut self, node: &Node, name: &str) -> String {
        node.child(name)
            .map(|n| self.children(n))
            .unwrap_or_default()
    }

    fn elem(&mut self, n: &Node) -> String {
        match n.local() {
            // Property bags and control characters carry no content.
            "rPr" | "ctrlPr" | "fPr" | "sSupPr" | "sSubPr" | "sSubSupPr" | "sPrePr" | "radPr"
            | "naryPr" | "dPr" | "mPr" | "accPr" | "barPr" | "funcPr" | "limLowPr" | "limUppPr"
            | "groupChrPr" | "boxPr" | "borderBoxPr" | "eqArrPr" | "phantPr" | "oMathParaPr"
            | "bookmarkStart" | "bookmarkEnd" | "proofErr" => String::new(),
            "r" => self.run(n),
            "t" => text_to_tex(&n.text()),
            "oMath" | "oMathPara" | "e" | "num" | "den" | "sub" | "sup" | "deg" | "lim"
            | "fName" | "box" | "phant" => self.children(n),
            "f" => {
                let num = self.arg(n, "num");
                let den = self.arg(n, "den");
                match n.path(&["fPr", "type"]).and_then(|t| t.attr("val")) {
                    Some("lin") => format!("{}/{}", wrap_if_needed(&num), wrap_if_needed(&den)),
                    Some("noBar") => format!("\\binom{{{num}}}{{{den}}}"),
                    _ => format!("\\frac{{{num}}}{{{den}}}"),
                }
            }
            "sSup" => {
                let base = self.arg(n, "e");
                let sup = self.arg(n, "sup");
                // Primes are superscripts in OMML and plain `'` in LaTeX.
                if !sup.is_empty() && sup.chars().all(|c| c == '\'') {
                    format!("{base}{sup}")
                } else {
                    format!("{}^{{{}}}", base_of(&base), sup)
                }
            }
            "sSub" => {
                let base = self.arg(n, "e");
                format!("{}_{{{}}}", base_of(&base), self.arg(n, "sub"))
            }
            "sSubSup" => {
                let base = self.arg(n, "e");
                format!(
                    "{}_{{{}}}^{{{}}}",
                    base_of(&base),
                    self.arg(n, "sub"),
                    self.arg(n, "sup")
                )
            }
            "sPre" => format!(
                "{{}}_{{{}}}^{{{}}}{}",
                self.arg(n, "sub"),
                self.arg(n, "sup"),
                base_of(&self.arg(n, "e"))
            ),
            "rad" => {
                let body = self.arg(n, "e");
                let hide = n
                    .path(&["radPr", "degHide"])
                    .is_some_and(|d| !matches!(d.attr("val"), Some("0") | Some("off")));
                let deg = self.arg(n, "deg");
                if hide || deg.is_empty() {
                    format!("\\sqrt{{{body}}}")
                } else {
                    format!("\\sqrt[{deg}]{{{body}}}")
                }
            }
            "nary" => self.nary(n),
            "d" => self.delim(n),
            "m" => format!("\\begin{{matrix}}{}\\end{{matrix}}", self.matrix_rows(n)),
            "acc" => {
                let chr = n
                    .path(&["accPr", "chr"])
                    .and_then(|c| c.attr("val"))
                    .unwrap_or("\u{302}");
                let body = self.arg(n, "e");
                let cmd = match chr.chars().next().unwrap_or('\u{302}') {
                    '\u{302}' | '^' => "\\hat",
                    '\u{303}' | '~' => "\\tilde",
                    '\u{304}' | '\u{305}' | '¯' | '‾' => "\\bar",
                    '\u{20D7}' | '→' => "\\vec",
                    '\u{307}' | '˙' => "\\dot",
                    '\u{308}' | '¨' => "\\ddot",
                    '\u{30C}' | 'ˇ' => "\\check",
                    '\u{301}' | '´' => "\\acute",
                    '\u{300}' | '`' => "\\grave",
                    '\u{306}' | '˘' => "\\breve",
                    _ => {
                        self.unknown.push(format!("accent {chr}"));
                        "\\hat"
                    }
                };
                format!("{cmd}{{{body}}}")
            }
            "bar" => {
                let body = self.arg(n, "e");
                if n.path(&["barPr", "pos"]).and_then(|p| p.attr("val")) == Some("top") {
                    format!("\\overline{{{body}}}")
                } else {
                    format!("\\underline{{{body}}}")
                }
            }
            "borderBox" => format!("\\boxed{{{}}}", self.arg(n, "e")),
            "func" => {
                let name = self.arg(n, "fName");
                let body = self.arg(n, "e");
                if body.is_empty() {
                    name
                } else {
                    format!("{name} {body}")
                }
            }
            "limLow" | "limUpp" => {
                let base = self.arg(n, "e");
                let lim = self.arg(n, "lim");
                let is_op = matches!(
                    base.as_str(),
                    "\\lim" | "\\max" | "\\min" | "\\sup" | "\\inf" | "\\limsup" | "\\liminf"
                ) || base.starts_with("\\lim");
                if n.is("limLow") {
                    if is_op {
                        format!("{base}_{{{lim}}}")
                    } else {
                        format!("\\underset{{{lim}}}{{{base}}}")
                    }
                } else if is_op {
                    format!("{base}^{{{lim}}}")
                } else {
                    format!("\\overset{{{lim}}}{{{base}}}")
                }
            }
            "groupChr" => {
                let chr = n
                    .path(&["groupChrPr", "chr"])
                    .and_then(|c| c.attr("val"))
                    .unwrap_or("\u{23DF}");
                let body = self.arg(n, "e");
                match chr {
                    "\u{23DE}" => format!("\\overbrace{{{body}}}"),
                    "\u{23DF}" => format!("\\underbrace{{{body}}}"),
                    "\u{2192}" => format!("\\xrightarrow{{{body}}}"),
                    "\u{2190}" => format!("\\xleftarrow{{{body}}}"),
                    _ => {
                        self.unknown.push(format!("groupChr {chr}"));
                        body
                    }
                }
            }
            "eqArr" => {
                let rows: Vec<String> = n
                    .children_named("e")
                    .map(|e| self.children(e))
                    .collect::<Vec<_>>();
                format!("\\begin{{aligned}}{}\\end{{aligned}}", rows.join(" \\\\ "))
            }
            other => {
                self.unknown.push(other.to_string());
                let text = n.text();
                text_to_tex(&text)
            }
        }
    }

    fn run(&mut self, r: &Node) -> String {
        let text: String = r.children_named("t").map(|t| t.text()).collect();
        let rpr = r.child("rPr");
        let normal = rpr.is_some_and(|p| p.child("nor").is_some());
        let sty = rpr.and_then(|p| p.child_val("sty"));
        if normal {
            return format!("\\text{{{}}}", text.replace(['{', '}'], ""));
        }
        let tex = text_to_tex(&text);
        // A multi-letter upright word is a function name or an operator.
        if sty == Some("p") && text.chars().count() > 1 && text.chars().all(char::is_alphabetic) {
            if FUNCTIONS.contains(&text.as_str()) {
                return format!("\\{text}");
            }
            return format!("\\mathrm{{{text}}}");
        }
        if sty.is_none() && FUNCTIONS.contains(&text.as_str()) && text.len() > 1 {
            return format!("\\{text}");
        }
        let font = match rpr.and_then(|p| p.child_val("scr")) {
            Some("double-struck") => Some("\\mathbb"),
            Some("script") => Some("\\mathcal"),
            Some("fraktur") => Some("\\mathfrak"),
            Some("sans-serif") => Some("\\mathsf"),
            Some("monospace") => Some("\\mathtt"),
            _ => None,
        };
        if let Some(cmd) = font {
            return format!("{cmd}{{{tex}}}");
        }
        match sty {
            Some("b") => format!("\\mathbf{{{tex}}}"),
            Some("bi") => format!("\\boldsymbol{{{tex}}}"),
            Some("p") if text.chars().count() == 1 && text.chars().all(char::is_alphabetic) => {
                format!("\\mathrm{{{tex}}}")
            }
            _ => tex,
        }
    }

    fn nary(&mut self, n: &Node) -> String {
        let pr = n.child("naryPr");
        let chr = pr
            .and_then(|p| p.child("chr"))
            .and_then(|c| c.attr("val"))
            .unwrap_or("\u{222B}");
        let hidden = |name: &str| {
            pr.and_then(|p| p.child(name))
                .is_some_and(|h| !matches!(h.attr("val"), Some("0") | Some("off")))
        };
        let op = match chr {
            "\u{2211}" => "\\sum",
            "\u{220F}" => "\\prod",
            "\u{2210}" => "\\coprod",
            "\u{222B}" => "\\int",
            "\u{222C}" => "\\iint",
            "\u{222D}" => "\\iiint",
            "\u{222E}" => "\\oint",
            "\u{22C3}" => "\\bigcup",
            "\u{22C2}" => "\\bigcap",
            "\u{22C1}" => "\\bigvee",
            "\u{22C0}" => "\\bigwedge",
            "\u{2A01}" => "\\bigoplus",
            "\u{2A02}" => "\\bigotimes",
            other => {
                self.unknown.push(format!("n-ary {other}"));
                "\\sum"
            }
        };
        let mut s = op.to_string();
        let sub = self.arg(n, "sub");
        let sup = self.arg(n, "sup");
        if !hidden("subHide") && !sub.is_empty() {
            s.push_str(&format!("_{{{sub}}}"));
        }
        if !hidden("supHide") && !sup.is_empty() {
            s.push_str(&format!("^{{{sup}}}"));
        }
        let body = self.arg(n, "e");
        if !body.is_empty() {
            s.push(' ');
            s.push_str(&body);
        }
        s
    }

    fn delim(&mut self, n: &Node) -> String {
        let pr = n.child("dPr");
        let get = |name: &str, default: &'static str| -> String {
            match pr.and_then(|p| p.child(name)) {
                Some(c) => c.attr("val").unwrap_or("").to_string(),
                None => default.to_string(),
            }
        };
        let beg = get("begChr", "(");
        let end = get("endChr", ")");
        let sep = get("sepChr", "|");
        let items: Vec<&Node> = n.children_named("e").collect();
        // A matrix inside delimiters is one of the bracketed environments; a bar-less
        // fraction inside parentheses is a binomial coefficient.
        if let [only] = items.as_slice() {
            let inner: Vec<&Node> = only.elems().filter(|e| !e.is("ctrlPr")).collect();
            if let [f] = inner.as_slice() {
                let no_bar = f.is("f")
                    && f.path(&["fPr", "type"]).and_then(|t| t.attr("val")) == Some("noBar");
                if no_bar && beg == "(" && end == ")" {
                    return self.elem(f);
                }
            }
            if let [m] = inner.as_slice() {
                if m.is("m") {
                    let env = match (beg.as_str(), end.as_str()) {
                        ("(", ")") => Some("pmatrix"),
                        ("[", "]") => Some("bmatrix"),
                        ("{", "}") => Some("Bmatrix"),
                        ("|", "|") => Some("vmatrix"),
                        ("‖", "‖") => Some("Vmatrix"),
                        _ => None,
                    };
                    if let Some(env) = env {
                        let rows = self.matrix_rows(m);
                        return format!("\\begin{{{env}}}{rows}\\end{{{env}}}");
                    }
                }
            }
        }
        let parts: Vec<String> = items.iter().map(|e| self.children(e)).collect();
        let sep_tex = if sep == "|" {
            " \\mid ".to_string()
        } else {
            text_to_tex(&sep)
        };
        format!(
            "\\left{} {} \\right{}",
            delim_tex(&beg),
            parts.join(&sep_tex),
            delim_tex(&end)
        )
    }

    fn matrix_rows(&mut self, m: &Node) -> String {
        let rows: Vec<String> = m
            .children_named("mr")
            .map(|row| {
                row.children_named("e")
                    .map(|e| self.children(e))
                    .collect::<Vec<_>>()
                    .join(" & ")
            })
            .collect();
        rows.join(" \\\\ ")
    }
}

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "coth", "log", "ln", "lg", "exp", "lim", "max", "min", "sup", "inf", "det", "dim", "ker",
    "deg", "gcd", "arg", "Pr", "hom", "limsup", "liminf",
];

/// A delimiter character as `\left` / `\right` take it (`.` for none).
fn delim_tex(c: &str) -> String {
    match c {
        "" => ".".into(),
        "{" => "\\{".into(),
        "}" => "\\}".into(),
        "⟨" | "〈" => "\\langle".into(),
        "⟩" | "〉" => "\\rangle".into(),
        "‖" => "\\|".into(),
        "⌊" => "\\lfloor".into(),
        "⌋" => "\\rfloor".into(),
        "⌈" => "\\lceil".into(),
        "⌉" => "\\rceil".into(),
        other => other.into(),
    }
}

/// A script base that is more than one token keeps its braces.
fn base_of(base: &str) -> String {
    let single = base.chars().count() == 1
        || (base.starts_with('\\') && base[1..].chars().all(char::is_alphabetic) && base.len() > 1)
        || (base.starts_with('{') && base.ends_with('}'))
        || base.starts_with("\\left")
        || base.starts_with("\\mathrm");
    if single || base.is_empty() {
        base.to_string()
    } else {
        format!("{{{base}}}")
    }
}

fn wrap_if_needed(s: &str) -> String {
    if s.chars().count() <= 1 || s.chars().all(char::is_alphanumeric) {
        s.to_string()
    } else {
        format!("({s})")
    }
}

/// Unicode math text → LaTeX: Greek letters, operators and relations become commands,
/// LaTeX-special characters are escaped.
fn text_to_tex(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let piece: &str = match c {
            'α' => "\\alpha",
            'β' => "\\beta",
            'γ' => "\\gamma",
            'δ' => "\\delta",
            'ε' => "\\epsilon",
            'ϵ' => "\\epsilon",
            'ζ' => "\\zeta",
            'η' => "\\eta",
            'θ' => "\\theta",
            'ϑ' => "\\vartheta",
            'ι' => "\\iota",
            'κ' => "\\kappa",
            'λ' => "\\lambda",
            'μ' => "\\mu",
            'ν' => "\\nu",
            'ξ' => "\\xi",
            'π' => "\\pi",
            'ρ' => "\\rho",
            'σ' => "\\sigma",
            'ς' => "\\varsigma",
            'τ' => "\\tau",
            'υ' => "\\upsilon",
            'φ' => "\\phi",
            'ϕ' => "\\phi",
            'χ' => "\\chi",
            'ψ' => "\\psi",
            'ω' => "\\omega",
            'Γ' => "\\Gamma",
            'Δ' => "\\Delta",
            'Θ' => "\\Theta",
            'Λ' => "\\Lambda",
            'Ξ' => "\\Xi",
            'Π' => "\\Pi",
            'Σ' => "\\Sigma",
            'Υ' => "\\Upsilon",
            'Φ' => "\\Phi",
            'Ψ' => "\\Psi",
            'Ω' => "\\Omega",
            '∞' => "\\infty",
            '≤' => "\\leq",
            '≥' => "\\geq",
            '≠' => "\\neq",
            '≈' => "\\approx",
            '≡' => "\\equiv",
            '∼' => "\\sim",
            '≃' => "\\simeq",
            '∝' => "\\propto",
            '±' => "\\pm",
            '∓' => "\\mp",
            '×' => "\\times",
            '÷' => "\\div",
            '⋅' | '·' => "\\cdot",
            '∘' => "\\circ",
            '→' => "\\to",
            '←' => "\\leftarrow",
            '↔' => "\\leftrightarrow",
            '⇒' => "\\Rightarrow",
            '⇐' => "\\Leftarrow",
            '⇔' => "\\Leftrightarrow",
            '↦' => "\\mapsto",
            '∈' => "\\in",
            '∉' => "\\notin",
            '∋' => "\\ni",
            '⊂' => "\\subset",
            '⊆' => "\\subseteq",
            '⊃' => "\\supset",
            '⊇' => "\\supseteq",
            '∪' => "\\cup",
            '∩' => "\\cap",
            '∅' => "\\emptyset",
            '∀' => "\\forall",
            '∃' => "\\exists",
            '¬' => "\\neg",
            '∧' => "\\wedge",
            '∨' => "\\vee",
            '∂' => "\\partial",
            '∇' => "\\nabla",
            '∑' => "\\sum",
            '∏' => "\\prod",
            '∫' => "\\int",
            '∮' => "\\oint",
            '√' => "\\sqrt{}",
            '…' => "\\ldots",
            '⋯' => "\\cdots",
            '⋮' => "\\vdots",
            '⋱' => "\\ddots",
            'ℝ' => "\\mathbb{R}",
            'ℕ' => "\\mathbb{N}",
            'ℤ' => "\\mathbb{Z}",
            'ℚ' => "\\mathbb{Q}",
            'ℂ' => "\\mathbb{C}",
            'ℓ' => "\\ell",
            'ℏ' => "\\hbar",
            '′' => "'",
            '″' => "''",
            '−' => "-",
            '∗' => "*",
            '⟨' => "\\langle",
            '⟩' => "\\rangle",
            '{' => "\\{",
            '}' => "\\}",
            '#' => "\\#",
            '%' => "\\%",
            '&' => "&",
            '\u{2061}' | '\u{2062}' | '\u{2063}' | '\u{200B}' => "",
            '\u{A0}' => " ",
            _ => {
                out.push(c);
                continue;
            }
        };
        push(&mut out, piece);
    }
    out
}

/// Append `piece`, inserting a space when a command name would otherwise run into a
/// letter (`\alpha` + `x` → `\alpha x`, not `\alphax`).
fn push(out: &mut String, piece: &str) {
    if piece.is_empty() {
        return;
    }
    let ends_with_command = {
        let tail: String = out
            .chars()
            .rev()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        !tail.is_empty() && out[..out.len() - tail.len()].ends_with('\\')
    };
    if ends_with_command && piece.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.push(' ');
    }
    out.push_str(piece);
}

/// Collapse runs of spaces and trim.
fn tidy(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    let mut space = false;
    for c in tex.trim().chars() {
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
    use super::*;
    use crate::twin::xml::parse;

    fn tex(omml: &str) -> String {
        let xml = format!(
            r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">{omml}</m:oMath>"#
        );
        let (tex, unknown) = to_latex(&parse(xml.as_bytes()).unwrap());
        assert!(unknown.is_empty(), "unknown: {unknown:?}");
        tex
    }

    fn r(t: &str) -> String {
        format!("<m:r><m:t>{t}</m:t></m:r>")
    }

    #[test]
    fn fraction_and_scripts() {
        assert_eq!(
            tex(&format!(
                "<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>",
                r("a"),
                r("b")
            )),
            "\\frac{a}{b}"
        );
        assert_eq!(
            tex(&format!(
                "{}{}<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
                r("E"),
                r("="),
                r("mc"),
                r("2")
            )),
            "E={mc}^{2}"
        );
        assert_eq!(
            tex(&format!(
                "<m:sSubSup><m:e>{}</m:e><m:sub>{}</m:sub><m:sup>{}</m:sup></m:sSubSup>",
                r("x"),
                r("i"),
                r("2")
            )),
            "x_{i}^{2}"
        );
    }

    #[test]
    fn radicals_with_and_without_degree() {
        assert_eq!(
            tex(&format!(
                r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>"#,
                r("π")
            )),
            "\\sqrt{\\pi}"
        );
        assert_eq!(
            tex(&format!(
                "<m:rad><m:deg>{}</m:deg><m:e>{}</m:e></m:rad>",
                r("3"),
                r("x")
            )),
            "\\sqrt[3]{x}"
        );
    }

    #[test]
    fn nary_sum_and_default_integral() {
        assert_eq!(
            tex(&format!(
                r#"<m:nary><m:naryPr><m:chr m:val="∑"/></m:naryPr><m:sub>{}</m:sub><m:sup>{}</m:sup><m:e>{}</m:e></m:nary>"#,
                r("i=1"),
                r("n"),
                r("i")
            )),
            "\\sum_{i=1}^{n} i"
        );
        assert_eq!(
            tex(&format!(
                "<m:nary><m:sub>{}</m:sub><m:sup>{}</m:sup><m:e>{}</m:e></m:nary>",
                r("-∞"),
                r("∞"),
                r("f")
            )),
            "\\int_{-\\infty}^{\\infty} f"
        );
    }

    #[test]
    fn delimiters_and_matrices() {
        assert_eq!(
            tex(&format!("<m:d><m:e>{}</m:e></m:d>", r("x+1"))),
            "\\left( x+1 \\right)"
        );
        let m = format!(
            "<m:m><m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr><m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr></m:m>",
            r("a"),
            r("b"),
            r("c"),
            r("d")
        );
        assert_eq!(
            tex(&format!(
                r#"<m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/></m:dPr><m:e>{m}</m:e></m:d>"#
            )),
            "\\begin{bmatrix}a & b \\\\ c & d\\end{bmatrix}"
        );
        assert_eq!(
            tex(&format!(
                r#"<m:d><m:dPr><m:begChr m:val="{{"/><m:endChr m:val=""/></m:dPr><m:e>{}</m:e></m:d>"#,
                r("x")
            )),
            "\\left\\{ x \\right."
        );
    }

    #[test]
    fn accents_functions_and_limits() {
        assert_eq!(
            tex(&format!(
                r#"<m:acc><m:accPr><m:chr m:val="⃗"/></m:accPr><m:e>{}</m:e></m:acc>"#,
                r("v")
            )),
            "\\vec{v}"
        );
        assert_eq!(
            tex(&format!(
                r#"<m:func><m:fName><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e>{}</m:e></m:func>"#,
                r("θ")
            )),
            "\\sin \\theta"
        );
        assert_eq!(
            tex(&format!(
                r#"<m:func><m:fName><m:limLow><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>lim</m:t></m:r></m:e><m:lim>{}</m:lim></m:limLow></m:fName><m:e>{}</m:e></m:func>"#,
                r("n→∞"),
                r("a")
            )),
            "\\lim_{n\\to\\infty} a"
        );
    }

    #[test]
    fn unknown_elements_keep_their_text_and_are_reported() {
        let xml = r#"<m:oMath xmlns:m="m"><m:weird><m:r><m:t>q</m:t></m:r></m:weird></m:oMath>"#;
        let (tex, unknown) = to_latex(&parse(xml.as_bytes()).unwrap());
        assert_eq!(tex, "q");
        assert_eq!(unknown, vec!["weird".to_string()]);
    }
}
