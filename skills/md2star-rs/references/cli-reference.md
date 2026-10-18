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

## `md2star-rs docx` — and the `md2docx` binary

```
md2star-rs docx <INPUT.md> [-o|--output OUT.docx] [--reference-doc TEMPLATE.docx]
md2docx         <INPUT.md> [-o|--output OUT.docx] [--reference-doc TEMPLATE.docx]
```

- `--output` defaults to the input path with its extension swapped for
  `.docx`, so `report.md` becomes `report.docx` beside it.
- `--reference-doc` styles the output after an existing Word file: its styles,
  theme, fonts and page setup are inherited, and headings and quotes use its
  named styles. This is Pandoc's flag of the same name, and it is how a house
  template gets applied without hand-editing anything afterwards.

## `md2star-rs pptx` — and the `md2pptx` binary

```
md2star-rs pptx <INPUT.md> [-o|--output OUT.pptx]
md2pptx         <INPUT.md> [-o|--output OUT.pptx]
```

Each `#` heading starts a slide; the body flattens to bullets. Ordered lists
become numbered, nested lists become sub-bullets, and quotes, code blocks and
tables become bullet lines. Inline formatting is flattened. There is no
`--reference-doc` equivalent for decks.

## Output contract

- Success: `wrote <path>` on **stdout**, exit `0`.
- Failure: the error on **stderr**, prefixed with the binary's own name,
  non-zero exit.

## What will not work, by design

No citations, no math, no embedded images, no hyperlink relations, no PDF, and
no reading a document back into Markdown. Those live in the Python `md2star`.
They are scope decisions, not flags waiting to be discovered.
