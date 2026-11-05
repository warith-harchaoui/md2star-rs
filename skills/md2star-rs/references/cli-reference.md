# CLI reference — `md2star-rs`, `md2docx`, `md2pptx`

`cargo install md2star-rs` installs three binaries.

## Which one to use

**`md2star-rs`** is named after the crate and therefore cannot collide with
anything. Use it whenever you write a command someone else will run: a script,
a Dockerfile, a CI job, a README.

**`md2docx` and `md2pptx`** carry the same names the Python `md2star` installs.
With both on one machine, `PATH` order decides which runs and nothing tells you
which it was. They are kept because removing or renaming them would break every
existing install — not because they are the better choice.

All three delegate to the same library calls. The output is byte-for-byte
identical; only the certainty differs.

## The command tree

```
md2star-rs docx      <INPUT.md> [common flags] [--csl FILE]     (= md2docx)
md2star-rs pptx      <INPUT.md> [common flags] [deck flags]     (= md2pptx)
md2star-rs pdf       <INPUT.md> [common flags]
md2star-rs twin      <INPUT.docx|INPUT.pptx> [--out DIR] [--no-images]
md2star-rs audit     <DECK.pptx> [--json] [--strict] [--min-pt N]
md2star-rs templates list | export <docx|pptx> <OUT> | layouts [TEMPLATE.pptx]
```

Common flags, aligned on the Python `md2star <cmd> --help` where they mean
something without Pandoc: `-o/--output`, `--author`, `--bib FILE` (repeatable),
`--bibliography-name`, `--citation-style author-date|numeric`, `--lang`,
`--date`, `--reference-doc PATH`, `--toc`, `--offline`,
`--allow-remote-images`, `-v/--verbose`, `-q/--quiet`.

Deck flags: `--template PATH` (wins over `--reference-doc`), `--min-pt N`,
`--smart-layout`, `--llm-endpoint URL`, `--llm-model NAME`.

- `--output` defaults to the input path with its extension swapped, so
  `report.md` becomes `report.docx` beside it.
- `--reference-doc` styles a `.docx` after an existing Word file, as Pandoc
  does: its styles, theme, fonts, settings, page setup, headers and footers are
  kept; every paragraph and character style is taken from it by id or by name
  (a French Word's `Titre1` is its `heading 1`), and a style it lacks is added.
- For a deck, `#`/`##` and `---` cut slides and each slide lands on a layout of
  the template (the bundled default, or `--reference-doc` / `--template`): an
  explicit `{layout="Name"}`, a role class (`.statement`, `.bignumber`,
  `.quote`, `.recap`, `.agenda`, `.section`, `.closing`, `.cover`, `.photo`), a
  recognised form, else the best measured fit. `::: notes` go to the notes pane.
- `--min-pt N` sets nothing under N points: a slide that no layout holds is split
  into "(cont.)" / "(suite)" slides, never shrunk.
- `--smart-layout` (build feature `llm`) lets the model at `--llm-endpoint` (or
  `MD2STAR_LLM_ENDPOINT`; `--llm-model`, default `best`) break near-ties between
  layouts that fit. Without the feature or the endpoint it warns and the
  deterministic choice stands.

## `audit`

`md2star-rs audit deck.pptx` reads any `.pptx` and prints one line per finding,
then a summary: `layout audit: clean (12 slides)` or `layout audit: 3 findings on
2 slides (1 overlap, 2 under-min-pt)`. Kinds, as in `md2star audit`: `off-slide`,
`overlap` (text on text, or a picture over text), `ghost-placeholder`,
`illegible-type` (under 12 pt), `under-min-pt` (with `--min-pt N`; footer chrome
exempt), `low-contrast` (a run that states its own colour, under the WCAG ratio
against the fill, shapes and backgrounds behind it: 3:1 for type of 18 pt, or 14 pt
bold, 4.5:1 otherwise; theme colours resolved through the master's colour map; a
picture or a gradient behind the text leaves it unjudged), `overrun` (an
observation; a failure with `--strict`). `--json` prints
the findings as an array of `{slide, kind, detail, shape[, measured_pt,
fits_pt]}`. Exit `0` when nothing but observations were found, `1` on a defect,
`2` when the deck cannot be read.

## `templates`

`templates list` shows the bundled templates, `templates export pptx out.pptx`
writes one out, and `templates layouts [deck.pptx]` lists a template's layouts
with the role md2star-rs reads in each and every placeholder's box and size.

## `docx`

What `md2star docx` (Pandoc underneath) does, in process. Without
`--reference-doc`, md2star's own `template.docx` (compiled into the binary) styles
the document, as it does for `md2star docx`.

- **Title block** from the front matter (`title`, `subtitle`, `author`, `date`,
  `abstract`) in the `Title`, `Subtitle`, `Author`, `Date`, `Abstract` styles,
  also written to the document properties. Without a front-matter title the
  first `#` heading becomes the title, authors and date follow it (md2star's
  rule). `--author "A, B"` and `--date` override the front matter.
- **`--toc`** (or `toc: true`, `toc-depth`, `toc-title`): a Word table-of-contents
  field Word rebuilds with page numbers on opening, pre-filled with linked entries
  for readers that do not update fields.
- **Images**: PNG, JPEG, GIF, BMP, TIFF embedded; SVG embedded with the PNG
  fallback Word requires (rasterised in process, feature `svg`); WebP and ICO
  are not embedded (Word cannot show them) and warn. Sized to the text column,
  `{width=50%}` / `{width=3in}` honoured, alt text kept, captioned figures centred.
- **Equations**: native Word equations (OMML) from the LaTeX; an unknown command
  stays readable and warns.
- **Citations**: `--bib FILE` (or `bibliography:`) with `--citation-style
  author-date|numeric`, `citation-style:`, or the *name* of a CSL file
  (`--csl ieee.csl`, `csl:`) — the file itself is never read. Author-date is
  Chicago (Pandoc's default), numeric is IEEE. The list goes under
  `--bibliography-name`, `reference-section-title`, or the localised
  "Bibliography".
- **Links** are real hyperlinks; `#id` links jump to headings, figures, tables.
- `---` is a page break (md2star's rule); footnotes and `^[inline notes]` are
  Word footnotes; tables keep their column alignment and caption.
- **Mermaid** blocks become centred pictures (SVG with its PNG fallback) with the
  `mermaid` feature; without it they stay code blocks with a warning. The same
  holds in decks and PDFs.

## What a build cannot do — and how that shows

A reference document given to `pdf` produces a warning on stderr (the PDF has
its own typography); the conversion still runs. A format this build was
compiled without (`pdf` without the feature) exits `1` with `not supported: …`
naming the missing feature.

Pandoc pass-through flags, `--skip-phase`, `--lint` and
`--no-remote-templates` do not apply to an in-process converter with no Pandoc
and no remote templates, and are not offered.

## `pdf` — in process, through Typst

```
md2star-rs pdf report.md                 # → report.pdf
md2star-rs pdf report.md --bib refs.bib --toc --lang fr-FR
```

Needs a build with the `pdf` feature (`cargo install md2star-rs --features pdf`,
Rust 1.92 or newer); without it the command says so and exits `1`. The PDF is
typeset by Typst inside the process with fonts compiled into the binary, so it
is the same file on every machine and every run. `--reference-doc` does not
apply: the PDF has its own typography. Formulas are typeset natively; a LaTeX
command outside the supported subset is shown as text with a warning. With the
`mermaid` feature, Mermaid blocks become diagrams; without it they stay code with
a warning.

## `twin` — a document back to Markdown

```
md2star-rs twin report.docx              # → report.md + report_media/
md2star-rs twin deck.pptx --out notes/   # → notes/deck.md + notes/deck_media/
md2star-rs twin report.docx --no-images  # prose and tables only
```

The Markdown is written next to the input (or into `--out`) as `<stem>.md`;
pictures go to `<stem>_media/` and are linked relatively. It reads the XML
inside the file, so it works on any `.docx` / `.pptx`, not only those
md2star wrote: Word styles and outline levels give the headings,
`numbering.xml` the lists, the relationships the links and pictures; a deck
gives one section per slide with its notes as `::: notes`. Equations come back
as LaTeX. What has no Markdown form (a chart, SmartArt, an unknown equation
element) is reported on stderr, never invented. A `.pdf` is refused: reading
one back needs text extraction with layout analysis and OCR, which md2star-rs
does not carry.

## Output contract

- Success: `wrote <path>` on **stdout** (unless `-q`), exit `0`.
- Warnings: `md2star-rs: warning: …` on **stderr** (unless `-q`; `-v` adds
  the warning's code).
- Failure: the error on **stderr**, prefixed with the binary's own name,
  exit `1`.

## Out of scope, by design

The web GUI, the HTTP server, the MCP surface and the Python `doctor` /
cache commands: a cross-device library has no use for them. Reading a PDF back
into Markdown needs text extraction with layout analysis and OCR, which
md2star-rs does not do; `twin` reads `.docx` and `.pptx`.
