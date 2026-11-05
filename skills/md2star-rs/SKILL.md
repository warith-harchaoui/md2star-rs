---
name: md2star-rs
description: >-
  Markdown to DOCX, PPTX and PDF in pure Rust, and DOCX or PPTX back to Markdown, with no Pandoc, no LibreOffice and nothing shelled out; the core compiles for iOS and Android. Word output with citations from a .bib, native equations, embedded images and a reference doc template; decks on a template's own layouts, chosen deterministically, with --min-pt; PDF through Typst in memory (feature pdf); a twin that reads a docx or pptx back into editable Markdown; an audit of any pptx for overlaps, off-slide shapes, tiny type and low contrast. Use when the user says "convert this markdown to Word", "markdown to docx", "make a deck from this markdown", "markdown to PDF without LaTeX", "use our corporate template", "turn this pptx back into markdown", "audit this deck", "a single binary or a phone app with no Pandoc", or names cargo add md2star-rs, md2star-rs docx, md2docx or md2pptx. Do NOT use to read a PDF back into Markdown (no OCR), for a full CSL style, to author the Markdown itself, or for HTML output.
license: Apache-2.0
metadata:
  author: Warith Harchaoui
  contact: warithmetics@deraison.ai
  repository: https://github.com/warith-harchaoui/md2star-rs
  crate: https://crates.io/crates/md2star-rs
  language: rust
---

# md2star-rs

Markdown into Word, PowerPoint and PDF, and Word or PowerPoint back into
Markdown, in pure Rust. No Pandoc, no LibreOffice, no LaTeX, no Python, nothing
invoked as a subprocess: the same library runs in a server, a CLI, or an iOS or
Android app.

## The operations

| Intent | CLI | Library |
|--------|-----|---------|
| Markdown to Word | `md2star-rs docx in.md -o out.docx` | `convert(md, Format::Docx, &options)`, `markdown_to_docx_bytes` |
| ... styled after a template | `md2star-rs docx in.md --reference-doc house.docx` | `options.reference_doc`, `markdown_to_docx_bytes_with_reference` |
| ... with citations | `md2star-rs docx in.md --bib refs.bib` | `options.bibliography` |
| Markdown to PowerPoint | `md2star-rs pptx in.md --min-pt 24` | `convert(md, Format::Pptx, &options)` |
| Markdown to PDF (feature `pdf`) | `md2star-rs pdf in.md` | `convert(md, Format::Pdf, &options)` |
| Word or PowerPoint back to Markdown | `md2star-rs twin in.docx` | `twin::to_markdown` |
| Check a deck | `md2star-rs audit deck.pptx --json` | `audit::audit` |
| Inspect templates | `md2star-rs templates list`, `templates layouts deck.pptx` | `templates::bundled`, `pptx::template::inspect` |

Every library call works on bytes in memory and returns its warnings beside
the output; nothing touches the filesystem unless the caller does.

## Writing Rust against it

```rust
use md2star_rs::{convert, Format, Options};

let mut options = Options::default();
options.bibliography = vec!["refs.bib".into()];
options.reference_doc = Some(std::fs::read("house.docx")?);
let out = convert("# Title\n\nAs shown [@pearl2009].", Format::Docx, &options)?;
std::fs::write("out.docx", &out.bytes)?;
```

## Prefer the unambiguous binary

`cargo install md2star-rs` installs `md2star-rs`, `md2docx` and `md2pptx`. The
last two carry the names the Python `md2star` installs, so with both on one
machine `PATH` order decides which runs. **Write `md2star-rs docx` and
`md2star-rs pptx`** in any command someone else will run; the output is byte
for byte the same.

## What to tell the user up front

- `pdf` and `mermaid` are Cargo features, off by default: `cargo install
  md2star-rs --features pdf,mermaid` (Rust 1.92 or newer for `pdf`). Without
  them, `pdf` says so and Mermaid blocks stay code with a warning.
- Citations come in two built-in styles, Chicago author-date and IEEE numeric;
  a `--csl` file's name picks one, its content is not read.
- `twin` refuses a PDF: reading one back needs OCR, which this crate does not
  carry. The Python `md2star twin` does it.
- `--smart-layout` (feature `llm`) only breaks near-ties between layouts that
  already fit; the deterministic choice is complete without it.

## References

- `references/triggers.md` — the full catalogue, mirroring the root `TRIGGERS.md`.
- `references/cli-reference.md` — every subcommand, every flag, exit codes.
- `references/surfaces.md` — which surfaces exist here, and which do not.
