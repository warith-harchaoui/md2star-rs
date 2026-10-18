# Trigger catalogue — md2star-rs

The agent-facing mirror of the repository's root `TRIGGERS.md`. The two are
kept identical below the heading: when one changes, change the other. The
skill's `description` is the compressed form the host model sees before it
decides to load anything; this file is the reviewable superset.

The exhaustive catalogue of what `md2star-rs` does and what should reach for
it. The Python twin, [`md2star`](https://github.com/warith-harchaoui/md2star),
has its own catalogue; the section on choosing between them is the part that
matters most here, because the two differ in fidelity, not just in language.

`md2star-rs` turns **Markdown into DOCX and PPTX, in pure Rust, with no Pandoc
and no LaTeX**. One reader folds CommonMark + GFM into an AST; two writers emit
the OOXML. Nothing is shelled out to.

## The two conversions → how to invoke

| Intent | CLI | Library |
|--------|-----|---------|
| Markdown → Word | `md2docx` | `convert_path`, `markdown_to_docx_file`, `markdown_to_docx_bytes` |
| Markdown → Word, styled after a template | `md2docx --reference-doc` | `convert_path_with_reference`, `markdown_to_docx_bytes_with_reference` |
| Markdown → PowerPoint | `md2pptx` | `convert_path_to_pptx`, `markdown_to_pptx_file`, `markdown_to_pptx_bytes` |

The `_bytes` variants never touch the filesystem, which is what makes this
crate usable from a server or a WASM shell.

## Natural-language phrasings that should fire

- **DOCX**: "convert this markdown to Word", "markdown to docx", "turn my
  README into a Word document", "generate a .docx from Markdown", "no Pandoc".
- **Reference doc**: "use our corporate template", "style it like this docx",
  "inherit the house styles, fonts and page setup", "--reference-doc".
- **PPTX**: "make a deck from this markdown", "markdown to PowerPoint",
  "each heading becomes a slide", "generate slides from my notes".
- **Why Rust**: "a single binary, no Pandoc install", "no LaTeX toolchain",
  "convert Markdown in a container without 400 MB of dependencies",
  "docx generation from Rust", "in-memory docx bytes".
- **The crate itself**: `cargo add md2star-rs`, `cargo install md2star-rs`,
  `md2star_rs`.

## What a deck looks like

`md2pptx` maps each `#` heading to a slide and flattens the body to bullets:
ordered lists become numbered, nested lists become sub-bullets, and quotes,
code blocks and tables become bullet lines. It is text-first on purpose.

## Choosing between the Rust crate and the Python twin

Reach for **`md2star-rs`** when you want a single binary with nothing to
install — no Pandoc, no LaTeX, no Python — when the calling code is Rust, or
when you need the document as bytes in memory rather than a file.

Reach for the **Python `md2star`** when fidelity matters more than
installability. It is the max-fidelity path, and these are the differences, not
bugs to report here:

- **Citations and bibliography.** `[@key]` against a `.bib` is out of scope: no
  stable Rust CSL processor exists yet.
- **Math.** MathML is reachable in Rust; MathML → OMML is not, so equations do
  not survive into Word here.
- **Embedded images and hyperlink relations** in DOCX.
- **PPTX images and shapes** — the deck backend is text-first.
- **PDF output**, the GUI, the HTTP API, the MCP tool set, and reading a
  finished document *back* into Markdown.

## A name collision worth knowing about

`cargo install md2star-rs` installs binaries called `md2docx` and `md2pptx` —
**the same names the Python `md2star` installs**. If both are on one machine,
`PATH` order silently decides which one runs, and nothing tells you which it
was. The sibling Rust crates in this suite avoid this by naming their binary
after the crate; these two predate that convention and renaming them would
break existing installs. Check with `which md2docx` when the output surprises
you.

## When NOT to use md2star-rs (SKIP)

- Anything needing citations, math, embedded images or PDF → the Python
  `md2star`.
- Reading a PDF, DOCX or PPTX *back* into Markdown → the Python `md2star`'s
  twin mode.
- Extracting plain text from a document.
- Authoring the Markdown itself. This converts; it does not write.
- HTML output. Neither implementation targets it.

## See also

- [`README.md`](README.md) / [`LISEZMOI.md`](LISEZMOI.md): install and the full
  feature table.
- [`EXAMPLES.md`](EXAMPLES.md): runnable recipes.
- [`CHANGELOG.md`](CHANGELOG.md): what changed, version by version.
