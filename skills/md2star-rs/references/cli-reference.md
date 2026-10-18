# CLI reference — `md2docx` and `md2pptx`

Both installed by `cargo install md2star-rs`.

## Read this first: the names collide

These two binaries have **the same names the Python `md2star` installs**. With
both on one machine, `PATH` order decides which runs and nothing tells you
which it was. When the output looks wrong, run `which md2docx` before
debugging anything else.

The sibling Rust crates in this suite name their binary after the crate
(`audio-helper-rs`, `youtube-helper-rs`, …) precisely to avoid this. These two
predate that convention, and renaming them now would break existing installs.

## `md2docx`

```
md2docx <INPUT.md> [-o|--output OUT.docx] [--reference-doc TEMPLATE.docx]
```

- `--output` defaults to the input path with its extension swapped for
  `.docx`, so `report.md` becomes `report.docx` beside it.
- `--reference-doc` styles the output after an existing Word file: its styles,
  theme, fonts and page setup are inherited, and headings and quotes use its
  named styles. This is Pandoc's flag of the same name, and it is how a house
  template gets applied without hand-editing anything afterwards.

## `md2pptx`

```
md2pptx <INPUT.md> [-o|--output OUT.pptx]
```

Each `#` heading starts a slide; the body flattens to bullets. Ordered lists
become numbered, nested lists become sub-bullets, and quotes, code blocks and
tables become bullet lines. Inline formatting is flattened. There is no
`--reference-doc` equivalent for decks.

## Output contract

- Success: the written path on **stdout**, exit `0`.
- Failure: the error on **stderr**, non-zero exit.

## What will not work, by design

No citations, no math, no embedded images, no hyperlink relations, no PDF, and
no reading a document back into Markdown. Those live in the Python `md2star`.
They are scope decisions, not flags waiting to be discovered.
