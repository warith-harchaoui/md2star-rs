//! LaTeX → OMML: native Word (and PowerPoint) equations from the raw LaTeX the reader
//! keeps in [`crate::ast::Inline::Math`].
//!
//! # What this module must do
//!
//! Convert the common subset of LaTeX math to Office Math Markup Language, in pure
//! Rust: fractions (`\frac`, `\dfrac`, `\tfrac`), sub/superscripts, roots (`\sqrt`,
//! `\sqrt[n]`), n-ary operators with limits (`\sum`, `\prod`, `\int`, `\oint`, `\lim`),
//! matrices (`matrix`, `pmatrix`, `bmatrix`, `cases`, `aligned`), accents (`\hat`,
//! `\bar`, `\vec`, `\dot`, `\tilde`), Greek letters, operators and relations,
//! `\left…\right` delimiters, `\text`, `\mathbf`/`\mathrm`/`\mathbb`/`\mathcal`. What
//! falls outside the subset must degrade to readable linear text inside `<m:r>`, never
//! to an error that loses the formula.
//!
//! Inline math becomes an `<m:oMath>` inside the paragraph; display math an
//! `<m:oMathPara>` of its own. The same fragment is valid in PresentationML (wrapped in
//! `a14:m`), which is why the PPTX writer calls this function too.
//!
//! Owned by the DOCX port (see [`crate::docx`]).

use crate::error::{Error, Result};

/// Convert LaTeX math source (without its `$` delimiters) to an OMML fragment:
/// `<m:oMathPara>…</m:oMathPara>` when `display`, `<m:oMath>…</m:oMath>` otherwise.
///
/// # Errors
///
/// Today: always [`Error::Unsupported`] — the converter is not ported yet.
pub fn latex_to_omml(tex: &str, display: bool) -> Result<String> {
    let _ = (tex, display);
    Err(Error::unsupported("LaTeX → OMML equations"))
}
