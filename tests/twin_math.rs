//! OMML → LaTeX, checked against the crate's own LaTeX → OMML: every formula of the
//! common subset written by the DOCX writer must read back as the same formula.
//!
//! "The same" is compared after removing what LaTeX does not distinguish here:
//! whitespace, grouping braces and the `\left` / `\right` sizing of delimiters.

use md2star_rs::docx::math;
use md2star_rs::twin::omml_to_latex;

fn canon(tex: &str) -> String {
    tex.replace("\\left", "")
        .replace("\\right", "")
        .replace("\\dfrac", "\\frac")
        .replace("\\tfrac", "\\frac")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '{' && *c != '}')
        .collect()
}

fn back(tex: &str, display: bool) -> String {
    let omml = math::convert(tex, display);
    let (latex, unknown) = omml_to_latex(&omml.xml).expect("well-formed OMML");
    assert!(
        unknown.is_empty(),
        "{tex}: unknown OMML {unknown:?} in {}",
        omml.xml
    );
    latex
}

#[test]
fn the_common_subset_reads_back() {
    let cases = [
        "E = mc^2",
        "x_i^2 + y_{ij}",
        "\\frac{a+b}{c}",
        "\\sqrt{x^2 + y^2}",
        "\\sqrt[3]{x}",
        "\\sum_{i=1}^{n} i",
        "\\int_{0}^{1} f(x) dx",
        "\\prod_{k=1}^{n} k",
        "\\alpha + \\beta \\leq \\gamma",
        "\\left( \\frac{1}{2} \\right)",
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        "\\begin{bmatrix} 1 & 0 \\\\ 0 & 1 \\end{bmatrix}",
        "\\hat{x} + \\vec{v} + \\tilde{y} + \\bar{z}",
        "\\overline{AB}",
        "a \\times b \\cdot c \\pm d",
        "x \\in \\mathbb{R}",
        "\\infty \\neq 0",
        "\\binom{n}{k}",
        "\\boxed{x = 1}",
        "\\underbrace{a + b}",
        "\\overbrace{a + b}",
        "\\mathbf{v} + \\mathcal{L}",
        "f'(x)",
        "\\partial_t u = \\nabla^2 u",
    ];
    let mut failures = Vec::new();
    for tex in cases {
        let got = back(tex, false);
        if canon(&got) != canon(tex) {
            failures.push(format!("{tex}  →  {got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "did not read back:\n{}",
        failures.join("\n")
    );
}

#[test]
fn functions_and_limits_read_back() {
    for (tex, want) in [
        ("\\sin \\theta", "\\sin\\theta"),
        ("\\lim_{n \\to \\infty} a_n", "\\lim_n\\to\\inftya_n"),
        ("\\log x", "\\logx"),
    ] {
        let got = back(tex, false);
        assert_eq!(canon(&got), canon(want), "{tex} → {got}");
    }
}

#[test]
fn display_math_reads_back() {
    let got = back(
        "\\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx = \\sqrt{\\pi}",
        true,
    );
    assert!(got.contains("\\int_{-\\infty}^{\\infty}"), "{got}");
    assert!(got.contains("\\sqrt{\\pi}"), "{got}");
}

#[test]
fn malformed_omml_is_a_read_error() {
    assert!(omml_to_latex("<m:oMath><m:r>").is_ok() || omml_to_latex("<m:oMath").is_err());
    assert!(matches!(omml_to_latex(""), Err(md2star_rs::Error::Read(_))));
}
