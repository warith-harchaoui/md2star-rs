---
name: md2star-rs
description: >-
  Markdown to DOCX and PPTX in pure Rust, with no Pandoc, no LaTeX and nothing shelled out: one CommonMark plus GFM reader, two OOXML writers. Convert a file or a string, get a file or bytes in memory, and style the Word output after an existing docx template whose styles, theme, fonts and page setup are inherited (a reference doc). Each heading becomes a slide in the deck backend. Use when the user says "convert this markdown to Word", "markdown to docx", "make a deck from this markdown", "markdown to PowerPoint", "use our corporate template", "a single binary with no Pandoc install", or names cargo add md2star-rs, md2docx or md2pptx. Do NOT use when citations, math, embedded images, PDF output or reading a document back into Markdown are needed -- the Python twin md2star is the max-fidelity path for all of those. Do NOT use to author the Markdown itself, or for HTML output.
license: BSD-3-Clause
metadata:
  author: Warith Harchaoui
  contact: warithmetics@deraison.ai
  repository: https://github.com/warith-harchaoui/md2star-rs
  crate: https://crates.io/crates/md2star-rs
  language: rust
---

# md2star-rs

Markdown into Word and PowerPoint, in pure Rust. No Pandoc, no LaTeX, no
Python, nothing invoked as a subprocess — which is the whole reason the crate
exists: the Pandoc path is a 400 MB install before the first conversion.

## The two conversions

| Intent | CLI | Library |
|--------|-----|---------|
| Markdown to Word | `md2docx in.md -o out.docx` | `convert_path`, `markdown_to_docx_file`, `markdown_to_docx_bytes` |
| ... styled after a template | `md2docx in.md --reference-doc house.docx` | `convert_path_with_reference`, `markdown_to_docx_bytes_with_reference` |
| Markdown to PowerPoint | `md2pptx in.md -o out.pptx` | `convert_path_to_pptx`, `markdown_to_pptx_file`, `markdown_to_pptx_bytes` |

The `_bytes` variants never touch the filesystem, which is what makes this
usable from a server or a WASM shell.

## Writing Rust against it

```rust
use std::path::Path;

md2star_rs::markdown_to_docx_file("# Title\n\nHello.", Path::new("out.docx"))?;
let bytes = md2star_rs::markdown_to_docx_bytes("Hello **world**")?;

// Inherit a house template's styles, theme, fonts and page setup.
let template = std::fs::read("house.docx")?;
let styled = md2star_rs::markdown_to_docx_bytes_with_reference("# Title", &template)?;

// Each `#` heading becomes a slide.
let deck = md2star_rs::markdown_to_pptx_bytes("# Slide 1\n\n- point\n- point")?;
```

## Check this before debugging a surprising result

`cargo install md2star-rs` installs binaries named `md2docx` and `md2pptx` —
**the same names the Python `md2star` installs**. With both on one machine,
`PATH` order decides which runs, and nothing says which it was. When output
looks unlike what this crate should produce, run `which md2docx` first. The
sibling Rust crates here name their binary after the crate precisely to avoid
this; these two predate that convention, and renaming them would break
existing installs.

## What the deck backend does

`md2pptx` is text-first by design: each `#` heading starts a slide, ordered
lists become numbered, nested lists become sub-bullets, and quotes, code blocks
and tables are flattened into bullet lines. Inline formatting is flattened. No
images, no shapes.

## Where the Python twin is still the answer

These are scope decisions, not defects to report:

- **Citations and bibliography** — no stable Rust CSL processor exists yet.
- **Math into Word** — MathML is reachable in Rust, MathML to OMML is not.
- **Embedded images and hyperlink relations** in DOCX.
- **PDF output**, the GUI, the HTTP API, the MCP tool set, and reading a
  finished document *back* into Markdown.

If a task needs any of those, say so and point at `md2star` rather than trying
to work around it here.

## References

- `references/triggers.md` — the full catalogue, mirroring the root `TRIGGERS.md`.
- `references/cli-reference.md` — both binaries, every flag, exit codes.
- `references/surfaces.md` — which surfaces exist here, and which do not.
