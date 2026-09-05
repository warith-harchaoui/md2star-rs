# md2star-rs

[🇫🇷 LISEZMOI](LISEZMOI.md) · [🇬🇧 README](README.md)

**A pure-Rust Markdown → DOCX & PPTX writer. No Pandoc, no subprocess, no runtime dependency —
single static binaries that run on every OS and device.**

By [Warith HARCHAOUI](https://linkedin.com/in/warith-harchaoui)

`md2star-rs` is a spin-off of the Python [`md2star`](https://github.com/warith-harchaoui/md2star).
The original is a thin, excellent wrapper around **Pandoc**; this one keeps the same goal —
Markdown in, a faithful `.docx` out — but reaches it entirely in Rust:

```text
Markdown ──▶ reader (pulldown-cmark → AST) ──▶ writer (AST → docx-rs) ──▶ .docx
```

## Why a Rust spin-off?

`md2star` is thin *because Pandoc is thick*: the features live inside Pandoc, and Pandoc is
a ~100 MB Haskell binary you must install first. That is fine on a desktop or in CI, but it
rules out phones, locked-down machines, and WASM. `md2star-rs` trades Pandoc's breadth for
a **single self-contained binary** and full ownership of the OOXML it emits — which is why
it needs none of the original's post-hoc `styles.xml` surgery (`postprocess.py`).

## Install

```bash
# From source (needs a Rust toolchain — https://rustup.rs)
cargo install --path .
# or build a release binary
cargo build --release   # → target/release/md2docx
```

See [`scripts/brew.sh`](scripts/brew.sh) for a Homebrew-based Rust setup on macOS/Linux.

## Usage

```bash
md2docx report.md                              # → report.docx (next to the input)
md2docx report.md -o out/final.docx
md2docx report.md --reference-doc house.docx   # style the output after house.docx
md2pptx talk.md                                # → talk.pptx (each `#` heading = a slide)
```

`--reference-doc` inherits the template's styles, theme, fonts and page setup, and routes
headings/quotes through its named styles (`Heading1`…`Heading6`, `Quote`) — the same idea as
Pandoc's flag of the same name.

As a library:

```rust
use std::path::Path;
md2star_rs::markdown_to_docx_file("# Title\n\nHello.", Path::new("out.docx")).unwrap();
let bytes = md2star_rs::markdown_to_docx_bytes("Hello **world**").unwrap();

// Style the output after a reference template read from disk.
let template = std::fs::read("house.docx").unwrap();
let styled = md2star_rs::markdown_to_docx_bytes_with_reference("# Title", &template).unwrap();

// Or produce a PowerPoint deck — each `#` heading becomes a slide.
let deck = md2star_rs::markdown_to_pptx_bytes("# Slide 1\n\n- point\n- point").unwrap();
```

More recipes in [`EXAMPLES.md`](EXAMPLES.md).

## What works today (v0.4)

**Two backends** off one reader/AST: `md2docx` (DOCX) and `md2pptx` (PPTX). The DOCX table
below is the detailed surface; `md2pptx` maps each `#` heading to a slide and flattens the body
to bullets (ordered → numbered, nested → sub-bullets, quotes/code/tables → bullet lines).

| Markdown | DOCX output |
|---|---|
| Headings `#`–`######` | Bold, level-scaled paragraphs — or the template's `Heading1`…`Heading6` styles under `--reference-doc` |
| Reference template | **`--reference-doc template.docx`** — inherit styles/theme/fonts/page-setup; headings & quotes via named styles |
| Paragraphs, `**bold**`, `_italic_`, `` `code` `` | Runs with matching formatting |
| Bullet & ordered lists | **Native Word numbering** (`numbering.xml` + `numPr`); ordered lists restart at 1; nesting → indent levels |
| GFM pipe tables | Real Word tables (`<w:tbl>`) |
| Fenced code blocks | Monospace, line-preserving |
| Block quotes | Rendered inline (recursively) |
| Footnotes `[^x]` | **Real Word footnotes** (`word/footnotes.xml`) |
| Links / images | Visible text / alt placeholder |

**Idempotent by construction:** the same Markdown produces byte-identical output on every run —
`.docx` *and* `.pptx`, even across concurrent threads. DOCX ids come from per-document counters
(never a process-global one); PPTX output is re-packed with a fixed timestamp so `ppt-rs`'s
wall-clock stamp can't leak in.

## Scope & trade-offs versus Pandoc `md2star`

This is a focused first cut, not a Pandoc replacement. Deliberately **not yet** here — each
a clean follow-up, none a redesign:

- **Bibliography / citations** — no stable Rust CSL processor exists yet
  (`citeproc-rs` is WIP/nightly), so `[@key]` citations are out of scope for now.
- **Math → OMML** — `latex2mathml` gets us to MathML, but MathML→OMML has no Rust crate.
- **Hyperlink relations** and **embedded images** — the Python `md2star` remains the
  "max-fidelity" path for those (and for DOCX/PPTX with math and citations). The PPTX backend is
  intentionally text-first: it flattens inline formatting and does not yet embed images or shapes.

If you need those, use [`md2star`](https://github.com/warith-harchaoui/md2star). If you need
a zero-install single binary for straightforward Markdown → Word or PowerPoint, use this.

## Development

```bash
cargo test                                   # unit + integration + doc-tests
cargo fmt --check                            # formatting gate
cargo clippy --all-targets -- -D warnings    # lint gate (warnings are errors)
```

CI runs all three on Linux, macOS, and Windows.

## License

Apache-2.0 — see [`LICENSE`](LICENSE).
