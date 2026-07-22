# Examples

Copy-paste recipes for `md2star-rs`. Every snippet is self-contained.

## CLI: convert a file

```bash
md2docx report.md
# wrote report.docx
```

Choose the output path explicitly:

```bash
md2docx report.md -o build/report-final.docx
# wrote build/report-final.docx
```

Convert a whole folder (shell loop — no feature needed):

```bash
for f in docs/*.md; do md2docx "$f"; done
```

## CLI: style the output after a reference document

Pass an existing `.docx` whose look you want to reuse. The output inherits its styles, theme,
fonts and page setup, and headings/quotes are emitted through its named styles
(`Heading1`…`Heading6`, `Quote`):

```bash
md2docx report.md --reference-doc house-template.docx
# wrote report.docx  (styled like house-template.docx)
```

## Library: string → bytes, styled after a template

```rust
// Read the template `.docx` you want to inherit styling from.
let template = std::fs::read("house-template.docx").unwrap();
let bytes =
    md2star_rs::markdown_to_docx_bytes_with_reference("# Title\n\nBody.", &template).unwrap();
// A .docx is a zip, so the buffer starts with the zip magic.
assert_eq!(&bytes[..2], b"PK");
```

To convert a file on disk directly with a template, use `convert_path_with_reference`.

## CLI: Markdown → PowerPoint slides

Each level-1 heading (`#`) starts a slide; the body until the next `#` becomes its bullets:

```bash
md2pptx talk.md
# wrote talk.pptx
md2pptx talk.md -o build/keynote.pptx
# wrote build/keynote.pptx
```

Given this Markdown, `md2pptx` produces a two-slide deck ("Intro" and "Results"):

```markdown
# Intro

- why this matters
- what we tried

# Results

1. faster builds
2. fewer bugs
```

## Library: Markdown → PPTX bytes

```rust
let bytes = md2star_rs::markdown_to_pptx_bytes("# Slide\n\n- one\n- two").unwrap();
// A .pptx is a zip, so the buffer starts with the zip magic.
assert_eq!(&bytes[..2], b"PK");
```

`markdown_to_pptx_file` and `convert_path_to_pptx` are the on-disk counterparts.

## Library: string → file

```rust
use std::path::Path;

fn main() -> md2star_rs::Result<()> {
    let markdown = "# Report\n\nHello **world**, with `code` and _emphasis_.";
    md2star_rs::markdown_to_docx_file(markdown, Path::new("report.docx"))?;
    Ok(())
}
```

## Library: string → bytes (servers / WASM)

Handy when you never want to touch the disk (an HTTP handler, a browser build):

```rust
let bytes = md2star_rs::markdown_to_docx_bytes("# Hi\n\nBody.").unwrap();
// A .docx is a zip, so the buffer starts with the zip magic.
assert_eq!(&bytes[..2], b"PK");
// e.g. return `bytes` as an application/vnd.openxmlformats-...-document response.
```

## Library: inspect the parsed AST

The reader is public, so you can fold Markdown into the intermediate representation without
producing a `.docx` — useful for tests or a second backend:

```rust
use md2star_rs::ast::Block;

let blocks = md2star_rs::reader::parse("# Title\n\nHello.");
assert!(matches!(blocks[0], Block::Heading { level: 1, .. }));
assert!(matches!(blocks[1], Block::Paragraph(_)));
```

## A table round-trips to a real Word table

```rust
let md = "| A | B |\n|---|---|\n| 1 | 2 |";
let bytes = md2star_rs::markdown_to_docx_bytes(md).unwrap();
// The produced document.xml contains a `<w:tbl>` element (verified in tests/convert.rs).
```
