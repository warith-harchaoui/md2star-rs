//! Equations in DOCX: native OMML in the paragraph, compared with what Pandoc writes for
//! the same LaTeX.

use std::io::{Cursor, Read};

use md2star_rs::{convert, Format, Options};

fn document(md: &str) -> (String, Vec<md2star_rs::ast::Warning>) {
    let out = convert(md, Format::Docx, &Options::default()).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(out.bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    (xml, out.warnings)
}

/// The structure of an OMML fragment: property blocks and run properties removed,
/// adjacent runs merged, as in the comparison with Pandoc.
fn structure(omml: &str) -> String {
    let mut s = omml.to_string();
    // Drop <m:rPr>…</m:rPr> and every <m:xxxPr>…</m:xxxPr>.
    while let Some(start) = s.find("Pr>").and_then(|end| s[..end].rfind("<m:")) {
        let name_end = s[start..].find('>').unwrap() + start;
        let name = &s[start + 3..name_end];
        let close = format!("</m:{name}>");
        let Some(stop) = s[name_end..].find(&close) else {
            break;
        };
        s.replace_range(start..name_end + stop + close.len(), "");
    }
    // Merge text runs.
    let mut out = String::new();
    let mut text = String::new();
    let mut rest = s.as_str();
    while !rest.is_empty() {
        if let Some(r) = rest
            .strip_prefix("<m:r><m:t>")
            .or_else(|| rest.strip_prefix("<m:r><m:t xml:space=\"preserve\">"))
        {
            let end = r.find("</m:t></m:r>").unwrap();
            text.push_str(r[..end].trim());
            rest = &r[end + "</m:t></m:r>".len()..];
            continue;
        }
        if !text.is_empty() {
            out.push_str(&format!("[{text}]"));
            text.clear();
        }
        let next = rest[1..].find('<').map_or(rest.len(), |i| i + 1);
        out.push_str(&rest[..next]);
        rest = &rest[next..];
    }
    if !text.is_empty() {
        out.push_str(&format!("[{text}]"));
    }
    out.replace(" />", "/>").replace("<m:e></m:e>", "<m:e/>")
}

fn first_math(xml: &str) -> &str {
    let start = xml.find("<m:oMath>").unwrap();
    let end = xml[start..].find("</m:oMath>").unwrap() + start + "</m:oMath>".len();
    &xml[start..end]
}

#[test]
fn inline_math_sits_in_the_paragraph_display_math_is_a_math_paragraph() {
    let (xml, warnings) = document("Inline $E=mc^2$ and\n\n$$\\frac{a}{b}$$\n");
    assert!(warnings.is_empty());
    assert!(
        xml.contains("<w:t xml:space=\"preserve\">Inline </w:t></w:r><m:oMath>"),
        "{xml}"
    );
    assert!(xml.contains("<w:p><w:pPr><w:pStyle w:val=\"BodyText\"/></w:pPr><m:oMathPara><m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr><m:oMath><m:f>"), "{xml}");
    assert!(xml.contains("xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\""));
}

/// The OMML Pandoc 3.6 writes for the fixtures' formulas (tests/fixtures/math.md), copied
/// from its output. Ours must have the same structure.
#[test]
fn the_math_fixture_matches_pandoc() {
    let pandoc = [
        (
            "$E=mc^2$",
            r#"<m:oMath><m:r><m:t>E</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>=</m:t></m:r><m:r><m:t>m</m:t></m:r><m:sSup><m:e><m:r><m:t>c</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>"#,
        ),
        (
            r"$$ \frac{-b \pm \sqrt{b^2 - 4ac}}{2a} $$",
            r#"<m:oMath><m:f><m:fPr><m:type m:val="bar" /></m:fPr><m:num><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>−</m:t></m:r><m:r><m:t>b</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>±</m:t></m:r><m:rad><m:radPr><m:degHide m:val="on" /></m:radPr><m:deg /><m:e><m:sSup><m:e><m:r><m:t>b</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>−</m:t></m:r><m:r><m:t>4</m:t></m:r><m:r><m:t>a</m:t></m:r><m:r><m:t>c</m:t></m:r></m:e></m:rad></m:num><m:den><m:r><m:t>2</m:t></m:r><m:r><m:t>a</m:t></m:r></m:den></m:f></m:oMath>"#,
        ),
        (
            r"$$ e^{i \times \pi}+1 = 0 $$",
            r#"<m:oMath><m:sSup><m:e><m:r><m:t>e</m:t></m:r></m:e><m:sup><m:r><m:t>i</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>×</m:t></m:r><m:r><m:t>π</m:t></m:r></m:sup></m:sSup><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>+</m:t></m:r><m:r><m:t>1</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>=</m:t></m:r><m:r><m:t>0</m:t></m:r></m:oMath>"#,
        ),
        (
            r"$\begin{pmatrix} a & b \\ c & d \end{pmatrix}$",
            r#"<m:oMath><m:d><m:dPr><m:begChr m:val="(" /><m:sepChr m:val="" /><m:endChr m:val=")" /><m:grow /></m:dPr><m:e><m:m><m:mPr><m:baseJc m:val="center" /><m:plcHide m:val="on" /><m:mcs><m:mc><m:mcPr><m:mcJc m:val="center" /><m:count m:val="1" /></m:mcPr></m:mc><m:mc><m:mcPr><m:mcJc m:val="center" /><m:count m:val="1" /></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:mr><m:mr><m:e><m:r><m:t>c</m:t></m:r></m:e><m:e><m:r><m:t>d</m:t></m:r></m:e></m:mr></m:m></m:e></m:d></m:oMath>"#,
        ),
        (
            r"$\sum_{i=1}^{n} i$",
            r#"<m:oMath><m:nary><m:naryPr><m:chr m:val="∑" /><m:limLoc m:val="undOvr" /><m:subHide m:val="off" /><m:supHide m:val="off" /></m:naryPr><m:sub><m:r><m:t>i</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>=</m:t></m:r><m:r><m:t>1</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>i</m:t></m:r></m:e></m:nary></m:oMath>"#,
        ),
        (
            r"$\hat{x} + \mathbb{R}$",
            r#"<m:oMath><m:acc><m:accPr><m:chr m:val="̂" /></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc><m:r><m:rPr><m:sty m:val="p" /></m:rPr><m:t>+</m:t></m:r><m:r><m:rPr><m:sty m:val="p" /><m:scr m:val="double-struck" /></m:rPr><m:t>R</m:t></m:r></m:oMath>"#,
        ),
    ];
    for (tex, expected) in pandoc {
        let (xml, warnings) = document(&format!("{tex}\n"));
        assert!(warnings.is_empty(), "{tex}: {warnings:?}");
        assert_eq!(structure(first_math(&xml)), structure(expected), "{tex}");
    }
}

#[test]
fn every_formula_of_the_fixtures_converts_without_warning() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "math.md",
        "syntax.md",
        "comprehensive_document.md",
        "guide_complet_document_fr.md",
        "table_zoo.md",
    ] {
        let md = std::fs::read_to_string(dir.join(name)).unwrap();
        let mut options = Options::default();
        options.base_dir = Some(dir.clone());
        let out = convert(&md, Format::Docx, &options).unwrap();
        let math: Vec<_> = out
            .warnings
            .iter()
            .filter(|w| w.code == "math-unsupported")
            .collect();
        assert!(math.is_empty(), "{name}: {math:?}");
    }
}

#[test]
fn an_unknown_command_stays_readable_and_warns() {
    let (xml, warnings) = document("$\\frac{a}{\\weird b}$\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "math-unsupported");
    assert!(warnings[0].message.contains("\\weird"));
    assert!(xml.contains("<m:t>\\weird</m:t>"));
    assert!(
        xml.contains("<m:f>"),
        "the known structure around it survives"
    );
}
