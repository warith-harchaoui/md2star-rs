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
md2star-rs templates list | export <docx|pptx> <OUT>
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
- For a deck, each `#` heading starts a slide and the body flattens to bullets
  (the v0.4 writer, kept behind the default `legacy-pptx` feature).

## `docx`

What `md2star docx` (Pandoc underneath) does, in process:

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

## Not ported yet — and how that shows

The full port of md2star is in progress. A flag that is accepted but not
honoured yet (a deck's `--reference-doc`/`--template`,
`--min-pt`, `--smart-layout`) produces a warning on stderr naming it; the
conversion still runs. A subcommand that is not ported yet (`audit`,
`templates`) exits non-zero with `not ported yet: …`. Nothing is
ignored silently.

Pandoc pass-through flags, `--skip-phase`, `--lint` and
`--no-remote-templates` do not apply to an in-process converter with no Pandoc
and no remote templates, and are not offered.

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
