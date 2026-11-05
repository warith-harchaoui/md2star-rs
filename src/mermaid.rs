//! Mermaid diagrams → SVG, in process (feature `mermaid`).
//!
//! The Python md2star renders Mermaid with `mmdc`, which drives a headless Chromium
//! through Node: none of that exists on a phone. This module uses
//! [`mermaid-rs-renderer`](https://crates.io/crates/mermaid-rs-renderer) instead — a
//! pure-Rust parser, layout engine and SVG writer (flowchart, sequence, pie, class,
//! state, gantt, ER and more), without a browser.
//!
//! Evaluated on 2026-11-05 on the three diagrams md2star documents use most
//! (flowchart, sequence, pie): all three render cleanly, and the same source gives the
//! same SVG on one machine. Two caveats, both from the renderer and both said here
//! rather than hidden:
//!
//! - **Text is measured with the system's fonts** (through `fontdb`), with calibrated
//!   widths as the fallback. Box sizes therefore depend on which fonts the machine has:
//!   the SVG is deterministic on one machine, not across machines.
//! - **The renderer caches the measuring font** under `$XDG_CACHE_HOME/mmdr` (or
//!   `~/.cache/mmdr`) the first time it runs. Nothing else is written.
//!
//! Without the feature, writers keep the diagram as a readable code block and raise a
//! `mermaid-not-rendered` warning — see each writer.
//!
//! `merman` (a parity-focused Rust port) was the other candidate; it needs Rust 1.95,
//! above what this crate asks for.

use crate::error::{Error, Result};

/// A rendered diagram.
#[derive(Debug, Clone, PartialEq)]
pub struct MermaidSvg {
    /// The SVG document.
    pub svg: String,
    /// Its width in CSS pixels.
    pub width: f64,
    /// Its height in CSS pixels.
    pub height: f64,
}

/// Render Mermaid source to SVG.
///
/// # Errors
///
/// [`Error::Unsupported`] naming the parse error when the source is not a diagram the
/// renderer reads.
///
/// # Examples
///
/// ```
/// let svg = md2star_rs::mermaid::render_mermaid("flowchart LR\n  A --> B").unwrap();
/// assert!(svg.svg.starts_with("<svg"));
/// assert!(svg.width > 0.0 && svg.height > 0.0);
/// ```
pub fn render_mermaid(source: &str) -> Result<MermaidSvg> {
    let options = mermaid_rs_renderer::RenderOptions::mermaid_default();
    let svg = mermaid_rs_renderer::render_strict(source, options)
        .map_err(|e| Error::unsupported(format!("Mermaid diagram: {e}")))?;
    let (width, height) = svg_size(&svg);
    Ok(MermaidSvg { svg, width, height })
}

/// The root element's `width`/`height`, else its `viewBox`.
fn svg_size(svg: &str) -> (f64, f64) {
    let root = svg
        .find("<svg")
        .and_then(|i| svg[i..].find('>').map(|j| &svg[i..i + j]))
        .unwrap_or("");
    let attr = |name: &str| -> Option<String> {
        let key = format!(" {name}=\"");
        let start = root.find(&key)? + key.len();
        let end = root[start..].find('"')?;
        Some(root[start..start + end].to_string())
    };
    let num = |v: Option<String>| -> Option<f64> {
        v.and_then(|s| s.trim_end_matches("px").trim().parse().ok())
    };
    match (num(attr("width")), num(attr("height"))) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            let vb: Vec<f64> = attr("viewBox")
                .unwrap_or_default()
                .split([' ', ','])
                .filter_map(|p| p.parse().ok())
                .collect();
            if vb.len() == 4 {
                (vb[2], vb[3])
            } else {
                (400.0, 300.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_sequence_and_pie_render() {
        for src in [
            "flowchart LR\n  A[Markdown] --> B{Lecteur}\n  B -->|AST| C[DOCX]\n  B --> D[PDF]",
            "sequenceDiagram\n  participant U as Utilisateur\n  participant M as md2star\n  U->>M: convertir\n  M-->>U: doc.docx",
            "pie title Formats\n  \"DOCX\" : 45\n  \"PPTX\" : 35\n  \"PDF\" : 20",
        ] {
            let out = render_mermaid(src).unwrap();
            assert!(out.svg.contains("<svg"), "{src}");
            assert!(out.width > 10.0 && out.height > 10.0, "{src}: {out:?}");
        }
    }

    #[test]
    fn labels_survive_into_the_svg() {
        let out = render_mermaid("flowchart TD\n  A[Début] --> B[Fin]").unwrap();
        assert!(out.svg.contains("Début") && out.svg.contains("Fin"));
    }

    #[test]
    fn rendering_is_stable_on_one_machine() {
        let src = "flowchart LR\n  A --> B --> C";
        assert_eq!(render_mermaid(src).unwrap(), render_mermaid(src).unwrap());
    }

    #[test]
    fn nonsense_is_an_error_not_a_panic() {
        assert!(render_mermaid("this is not a diagram at all").is_err());
    }

    #[test]
    fn svg_size_reads_width_or_viewbox() {
        assert_eq!(svg_size(r#"<svg width="120" height="80">"#), (120.0, 80.0));
        assert_eq!(svg_size(r#"<svg viewBox="0 0 300 200">"#), (300.0, 200.0));
    }
}
