# TRIGGERS: md2star-rs

The exhaustive catalogue of what `md2star-rs` does and what should reach for
it. The Python twin, [`md2star`](https://github.com/warith-harchaoui/md2star),
has its own catalogue; the section on choosing between them says where the two
still differ.

`md2star-rs` turns **Markdown into DOCX, PPTX and PDF, and DOCX or PPTX back
into Markdown, in pure Rust**: no Pandoc, no LibreOffice, no LaTeX, nothing
shelled out. One reader folds CommonMark, GFM and Pandoc's extensions into an
AST; the writers emit the OOXML themselves, and Typst sets the PDF in memory.
The core compiles for iOS and Android.

## The operations → how to invoke

| Intent | CLI | Library |
|--------|-----|---------|
| Markdown → Word | `md2star-rs docx` (or `md2docx`) | `convert(…, Format::Docx, …)`, `markdown_to_docx_bytes` |
| Markdown → Word, styled after a template | `md2star-rs docx --reference-doc` | `Options::reference_doc`, `markdown_to_docx_bytes_with_reference` |
| Markdown → Word with citations | `md2star-rs docx --bib refs.bib` | `Options::bibliography` |
| Markdown → PowerPoint | `md2star-rs pptx` (or `md2pptx`) | `convert(…, Format::Pptx, …)` |
| Markdown → PDF (feature `pdf`) | `md2star-rs pdf` | `convert(…, Format::Pdf, …)` |
| Word / PowerPoint → Markdown | `md2star-rs twin` | `twin::to_markdown` |
| Check a deck | `md2star-rs audit` | `audit::audit` |
| Bundled templates, a template's layouts | `md2star-rs templates` | `templates::bundled`, `pptx::template::inspect` |

Every library call works in memory, which is what makes the crate usable from
a server, a phone app or a WASM shell.

## Natural-language phrasings that should fire

- **DOCX**: "convert this markdown to Word", "markdown to docx", "turn my
  README into a Word document", "with a table of contents", "keep my
  equations as real Word equations", "embed the images".
- **Citations**: "resolve [@key] against my .bib", "BibTeX to Word",
  "author-date citations", "IEEE numbered references".
- **Reference doc**: "use our corporate template", "style it like this docx",
  "inherit the house styles, fonts and page setup", "--reference-doc".
- **PPTX**: "make a deck from this markdown", "markdown to PowerPoint", "use
  the layouts of this template", "nothing under 24 pt", "speaker notes".
- **PDF**: "markdown to PDF without LaTeX", "a PDF on a phone", "the same PDF
  on every machine".
- **Twin**: "turn this docx back into markdown", "make an editable twin of
  this deck", "extract the slides' text and notes as markdown".
- **Audit**: "check this PowerPoint for overlapping text", "lint my deck",
  "is any text too small", "is this slide readable", "low contrast".
- **Why Rust**: "a single binary, no Pandoc install", "convert Markdown in an
  iOS or Android app", "no LibreOffice in the container", "docx generation
  from Rust", "in-memory docx bytes", "byte-identical output".
- **The crate itself**: `cargo add md2star-rs`, `cargo install md2star-rs`,
  `md2star_rs`, `md2star-rs docx`, `md2star-rs pptx`, `md2star-rs pdf`,
  `md2star-rs twin`, `md2star-rs audit`.

## Choosing between the Rust crate and the Python twin

Reach for **`md2star-rs`** when nothing may be installed beside it (no Pandoc,
no LibreOffice, no Python), when the code runs on a phone or a tablet, when the
calling code is Rust, when the document is wanted as bytes in memory, or when
the output must be byte-identical run after run.

Reach for the **Python `md2star`** for what this crate leaves out on purpose:

- **A full CSL style.** Two styles are built in (Chicago author-date, IEEE
  numeric); a `.csl` file's name picks one, its content is not read.
- **A PDF read back into Markdown**, which needs OCR.
- **Cloning a template's example slides** (`--reference-slides`) and the visual
  judge that looks at rendered slides.
- **The GUI, the HTTP API, the MCP tool set.**

## Three binaries: which one to reach for

`cargo install md2star-rs` installs **`md2star-rs`**, `md2docx` and `md2pptx`.

`md2docx` and `md2pptx` carry **the same names the Python `md2star` installs**.
With both on one machine, `PATH` order silently decides which runs and nothing
tells you which it was. They are kept because removing or renaming them would
break every existing install.

`md2star-rs docx` and `md2star-rs pptx` cannot collide with anything, and they
delegate to the same library calls, so the output is byte-for-byte identical.
**Use them in scripts, Dockerfiles and CI**: anywhere being certain which
implementation ran matters more than typing less.

## When NOT to use md2star-rs (SKIP)

- Reading a PDF back into Markdown → the Python `md2star twin`.
- A citation style that must follow a given `.csl` file exactly → the Python
  `md2star`.
- Extracting plain text from a document.
- Authoring the Markdown itself. This converts; it does not write.
- HTML output. Neither implementation targets it.

## See also

- [`README.md`](README.md) / [`LISEZMOI.md`](LISEZMOI.md): install, features,
  what works and the measured comparison.
- [`EXAMPLES.md`](EXAMPLES.md): runnable recipes.
- [`CHANGELOG.md`](CHANGELOG.md): what changed, version by version.
