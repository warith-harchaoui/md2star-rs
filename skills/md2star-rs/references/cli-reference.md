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
md2star-rs docx      <INPUT.md> [common flags]                  (= md2docx)
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
- `--reference-doc` styles a `.docx` after an existing Word file: its styles,
  theme, fonts and page setup are inherited, and headings, quotes, code and
  list items use its named styles.
- For a deck, each `#` heading starts a slide and the body flattens to bullets
  (the v0.4 writer, kept behind the default `legacy-pptx` feature).

## Not ported yet — and how that shows

The full port of md2star is in progress. A flag that is accepted but not
honoured yet (`--bib`, `--toc`, a deck's `--reference-doc`/`--template`,
`--min-pt`, `--smart-layout`) produces a warning on stderr naming it; the
conversion still runs. A subcommand that is not ported yet (`pdf`, `twin`,
`audit`, `templates`) exits non-zero with `not ported yet: …`. Nothing is
ignored silently.

Pandoc pass-through flags, `--skip-phase`, `--lint` and
`--no-remote-templates` do not apply to an in-process converter with no Pandoc
and no remote templates, and are not offered.

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
