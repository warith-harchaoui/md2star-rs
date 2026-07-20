# md2star-rs

[🇫🇷 LISEZMOI](LISEZMOI.md) · [🇬🇧 README](README.md)

**A pure-Rust Markdown → DOCX writer. No Pandoc, no subprocess, no runtime dependency — a
single static binary that runs on every OS and device.**

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
md2docx report.md                 # → report.docx (next to the input)
md2docx report.md -o out/final.docx
```

As a library:

```rust
use std::path::Path;
md2star_rs::markdown_to_docx_file("# Title\n\nHello.", Path::new("out.docx")).unwrap();
let bytes = md2star_rs::markdown_to_docx_bytes("Hello **world**").unwrap();
```

More recipes in [`EXAMPLES.md`](EXAMPLES.md).

## What works today (v0.1)

| Markdown | DOCX output |
|---|---|
| Headings `#`–`######` | Bold, level-scaled paragraphs |
| Paragraphs, `**bold**`, `_italic_`, `` `code` `` | Runs with matching formatting |
| Bullet & ordered lists | Marker-prefixed paragraphs |
| GFM pipe tables | Real Word tables (`<w:tbl>`) |
| Fenced code blocks | Monospace, line-preserving |
| Block quotes | Rendered inline (recursively) |
| Footnotes `[^x]` | Inline marker + trailing **Notes** section |
| Links / images | Visible text / alt placeholder |

## Scope & trade-offs versus Pandoc `md2star`

This is a focused first cut, not a Pandoc replacement. Deliberately **not yet** here — each
a clean follow-up, none a redesign:

- **Bibliography / citations** — no stable Rust CSL processor exists yet
  (`citeproc-rs` is WIP/nightly), so `[@key]` citations are out of scope for now.
- **Math → OMML** — `latex2mathml` gets us to MathML, but MathML→OMML has no Rust crate.
- **Real Word footnotes** (`footnotes.xml`) — v0.1 uses inline markers + a Notes section.
- **Native list numbering**, **hyperlink relations**, **embedded images**, **`--reference-doc`
  style inheritance**, and **PPTX** — the Python `md2star` remains the "max-fidelity" path
  for those; keep it for DOCX+PPTX+PDF with math and citations.

If you need those, use [`md2star`](https://github.com/warith-harchaoui/md2star). If you need
a zero-install single binary for straightforward Markdown → Word, use this.

## Development

```bash
cargo test                                   # unit + integration + doc-tests
cargo fmt --check                            # formatting gate
cargo clippy --all-targets -- -D warnings    # lint gate (warnings are errors)
```

CI runs all three on Linux, macOS, and Windows.

## License

Apache-2.0 — see [`LICENSE`](LICENSE).
