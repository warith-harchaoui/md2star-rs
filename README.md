# md2star-rs

[🇫🇷 LISEZMOI](LISEZMOI.md) · [🇬🇧 README](README.md)

[![crates.io](https://img.shields.io/crates/v/md2star-rs.svg)](https://crates.io/crates/md2star-rs)

**Markdown to Word, PowerPoint and PDF, and Word or PowerPoint back to Markdown, in pure
Rust. No Pandoc, no LibreOffice, no LaTeX, no subprocess: the same library runs on a
server, a laptop, an iPhone or an Android phone.**

By [Warith HARCHAOUI](https://linkedin.com/in/warith-harchaoui)

`md2star-rs` is the cross-device port of the Python
[`md2star`](https://github.com/warith-harchaoui/md2star). The Python version drives Pandoc,
LibreOffice and a language model; this one does the same work inside the process. Its core
compiles for iOS and Android, and the CLI ships as a single static binary.

```text
                                                  ┌──▶ docx ──▶ .docx
Markdown ──▶ reader ──▶ AST ──(BibTeX citations)──┼──▶ pptx ──▶ .pptx   (template layouts)
                                                  └──▶ pdf  ──▶ .pdf    (Typst, feature `pdf`)

.docx / .pptx ──▶ twin ──▶ Markdown            .pptx ──▶ audit ──▶ findings
```

## What works

| Output | What it does | Checked by |
|---|---|---|
| **DOCX** (`md2star-rs docx`) | Pandoc's WordprocessingML vocabulary, written directly: title block from the front matter, table of contents (`--toc`), headings with bookmarks, native list numbering, task boxes, tables with alignment and captions, real footnotes, real hyperlinks and `#id` jumps, embedded images (PNG, JPEG, GIF, BMP, TIFF, SVG with its PNG fallback) sized to the column, **native equations** (LaTeX → OMML), **citations and a bibliography** from a `.bib`, Mermaid diagrams as pictures (feature `mermaid`). md2star's own `template.docx` is the default; `--reference-doc` keeps every part of another one (styles, theme, numbering, headers, footers, page setup). | `tests/docx_*.rs`, `tests/convert.rs`, `tests/reference_doc.rs` open the zip and read the XML |
| **PPTX** (`md2star-rs pptx`) | PresentationML written on the template's own masters and layouts (md2star's `template.pptx` by default). Each slide fills the placeholders of a layout chosen **deterministically**: `{layout="Name"}`, a role class (`.statement` `.bignumber` `.quote` `.recap` `.agenda` `.section` `.closing`), a recognised form, else a fit score measured with the theme fonts' real widths. `--min-pt N` never sets text under N points: the slide moves to a roomier layout or is split. Pictures, tables, code, two columns, speaker notes, footnotes, equations (OMML), links. `--smart-layout` (feature `llm`) lets a model break near-ties only. | `tests/pptx_*.rs`, `tests/audit_decks.rs` |
| **PDF** (`md2star-rs pdf`, feature `pdf`) | Markdown → Typst markup → PDF, compiled in memory with fonts embedded in the binary: the same file on every machine and every run. Title block, headings, lists, tables, figures, footnotes, links, table of contents, citations, native math (LaTeX → Typst math). | `tests/pdf_render.rs` |
| **twin** (`md2star-rs twin`) | Any `.docx` or `.pptx` back to editable Markdown, pictures extracted to `<name>_media/`: headings, lists with their numbering, tables, links, footnotes, code, quotes, equations back to LaTeX, speaker notes. Charts and SmartArt are reported, never invented. | `tests/twin_*.rs`: round trips Markdown → DOCX/PPTX → Markdown, and md2star's own outputs |
| **audit** (`md2star-rs audit`) | Any `.pptx`: `off-slide`, `overlap` (text on text, picture over text), `ghost-placeholder`, `illegible-type`, `under-min-pt`, `low-contrast` (WCAG ratio against the resolved backdrop, theme colours included), `overrun`. Same kinds, JSON and exit codes as `md2star audit`. | `tests/audit_decks.rs` |
| **templates** (`md2star-rs templates`) | List or export the bundled templates; show the layouts and placeholders the slide planner reads in any `.pptx`. | `tests/templates_cli.rs` |

Every output is **byte-identical from one run to the next** (DOCX, PPTX and PDF). No input
makes a conversion fail on content: what cannot be done becomes a warning returned beside
the bytes.

## Cross-device, proven rather than claimed

CI runs `cargo check --lib` for `aarch64-apple-ios` and `aarch64-linux-android` on every
push, three times per target: bare (`--no-default-features`), with `svg`, then with
`svg,pdf,mermaid`. `cargo check` needs the target's standard library but no SDK, so an
Ubuntu runner proves it. The core has no C dependency. Typst brings one, `psm`, which
assembles a few lines with the target's C compiler (Xcode's or the NDK's in an app build).

The same Ubuntu job compiles and lints `--all-features`. No GPU path is relevant here:
conversion is XML, text measurement and typesetting on the CPU, so there is no acceleration
to enable on Ubuntu or anywhere else.

## Cargo features and what they cost

Sizes: the stripped release `md2star-rs` binary, macOS arm64, Rust 1.95, measured for this
release.

| Features | What they add | Binary | MSRV |
|---|---|---|---|
| `cli` | the `md2star-rs`, `md2docx` and `md2pptx` binaries (`clap`) | 3.6 MB | 1.88 |
| `cli`, `svg` (the default) | the PNG fallback Word needs under an SVG picture, rasterised by `resvg` | 6.4 MB | 1.88 |
| + `pdf` | in-process PDF through Typst, with its fonts (about 10 MB of them) | 50.4 MB | **1.92** |
| + `pdf`, `mermaid` | Mermaid diagrams drawn as SVG (`mermaid-rs-renderer`) in DOCX, PPTX and PDF | 52.9 MB | 1.92 |
| every feature | + `net` (fetch `http(s)://` images, still opt-in per run) and `llm` (`--smart-layout` through an OpenAI-compatible endpoint) | 54.7 MB | 1.92 |

The crate's MSRV is **1.88**; the `pdf` feature alone needs **1.92** (Typst 0.15's floor).
Both were checked on real 1.88.0 and 1.92.0 toolchains for this release.

## Install

```bash
cargo install md2star-rs                          # docx, pptx, twin, audit, templates
cargo install md2star-rs --features pdf,mermaid   # + PDF and diagrams (Rust 1.92+)
```

As a library:

```toml
[dependencies]
md2star-rs = "0.5"
# the core only (no CLI), as an iOS or Android app embeds it:
# md2star-rs = { version = "0.5", default-features = false }
```

## Usage

```bash
md2star-rs docx report.md                           # → report.docx
md2star-rs docx report.md --bib refs.bib --toc      # citations, table of contents
md2star-rs docx report.md --reference-doc house.docx
md2star-rs pptx talk.md --min-pt 24                 # → talk.pptx, nothing under 24 pt
md2star-rs pdf report.md                            # → report.pdf (feature `pdf`)
md2star-rs twin report.docx                         # → report.md + report_media/
md2star-rs audit talk.pptx --json                   # exit 1 on a defect
md2star-rs templates layouts house.pptx             # what the slide planner sees
```

Every option is in
[`skills/md2star-rs/references/cli-reference.md`](skills/md2star-rs/references/cli-reference.md).

```rust
use md2star_rs::{convert, Format, Options};

let mut options = Options::default();
options.bibliography = vec!["refs.bib".into()];
let out = convert("# Title\n\nAs shown [@pearl2009].", Format::Docx, &options)?;
std::fs::write("out.docx", &out.bytes)?;
for w in &out.warnings {
    eprintln!("{}: {}", w.code, w.message);
}

let twin = md2star_rs::twin::to_markdown(
    &std::fs::read("deck.pptx")?,
    md2star_rs::twin::SourceKind::Pptx,
    &Default::default(),
)?;
println!("{}", twin.markdown);
```

More in [`EXAMPLES.md`](EXAMPLES.md).

## Compared with md2star (Python), measured

| Point | Result | How |
|---|---|---|
| Citations | word for word what Pandoc 3.6.1 `--citeproc` writes, in Chicago author-date and in IEEE, for every entry of the fixture `.bib` | `tests/docx_citations.rs` |
| Equations (OMML) | 49 of 60 formulas have the exact structure Pandoc 3.6.1 writes. 10 of the other 11 differ on purpose (n-ary objects for `\prod` `\oint` `\bigcup` `\iint`, `limLow` for `\lim` `\max` `\operatorname*`, braces as `groupChr`, no empty trailing matrix row); `\big(` is set as plain text | a corpus of 62 formulas (the converter's own tests and the fixtures), compared after dropping properties and merging runs; Pandoc rejected 2 of them |
| Slide layouts | on the 10 hinted slides of `tests/fixtures/deck.md`, the layouts md2star 3.12 picks. md2star 3.13.1 agrees on 9 and puts a lone table on "Two Content", leaving one box empty | `tests/pptx_layout.rs`, re-run against `md2pptx --smart-layout --no-visual-confirm` |
| Audit | the findings of `md2star audit --min-pt 30 --json` (md2star 3.13.1) on the Pandoc-made decks, slide by slide and shape by shape | `tests/audit_decks.rs` |

## Out of scope, and why

- **PDF → Markdown.** `twin` reads `.docx` and `.pptx`. A PDF needs layout analysis and
  OCR, which nothing in pure Rust does well on a phone; it is refused with that reason.
- **Full CSL.** Two styles are built in, Chicago author-date and IEEE numeric. A `--csl`
  file's *name* picks one of them; its content is never read.
- **Mermaid across machines.** `mermaid-rs-renderer` measures text with the machine's
  fonts, so a diagram is stable run after run on one machine, not identical between two.
  It caches one font under `~/.cache/mmdr`.
- **`--reference-slides`** (cloning a template's own example slides) and md2star's
  **visual judge** (a vision model looking at rendered slides). The geometric `audit`
  replaces the second; neither fits a phone.
- **The GUI, the HTTP server, the MCP tools and `doctor`** of the Python version: a
  cross-device library has no use for them.

## Three binaries: which one to reach for

`cargo install md2star-rs` installs **`md2star-rs`**, `md2docx` and `md2pptx`. The last two
carry the names the Python `md2star` installs, so with both on one machine `PATH` order
decides which runs. Use `md2star-rs docx` and `md2star-rs pptx` in scripts, Dockerfiles and
CI: they cannot collide, and their output is byte for byte the same.

## Agent skill

`skills/md2star-rs/` packages this crate as a Claude Skill and an OpenCode skill
([`skills/README.md`](skills/README.md)). Its trigger catalogue, [`TRIGGERS.md`](TRIGGERS.md),
is mirrored in `references/triggers.md`; `tests/skill.rs` fails the build if they drift.

## Development

```bash
scripts/install-hooks.sh   # once per clone: the pre-push gate
```

The pre-push hook runs `cargo fmt --check`, clippy with warnings as errors (default, no
features, every feature), the tests (default, no features, every feature), `cargo doc` with
warnings as errors, the iOS and Android checks, then `scripts/check-fresh-resolve.sh` with
and without features: a build with no `Cargo.lock`, as a downstream `cargo add` gets it
(md2star-rs 0.4.0 once shipped uninstallable while every locked build was green). CI repeats
the light part on one Ubuntu job. No tracked file may hold a machine's path:
`tests/no_personal_paths.rs` enforces it.

## License

Apache-2.0, see [`LICENSE`](LICENSE).
