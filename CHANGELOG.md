# Changelog

All notable changes to `md2star-rs` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [0.1.0] — unreleased

Initial spin-off: a pure-Rust Markdown → DOCX writer, no Pandoc.

### Added
- `pulldown-cmark → AST → docx-rs` pipeline with a named intermediate representation
  (`ast::Block` / `ast::Inline`) as the reader/writer seam.
- Library API: `markdown_to_docx_bytes`, `markdown_to_docx_file`, `convert_path`, and the
  public `reader::parse`.
- `md2docx` CLI (`md2docx <input.md> [-o out.docx]`).
- Supported Markdown: headings, paragraphs, bold/italic/inline-code, bullet & ordered
  lists, GFM pipe tables, fenced code blocks, block quotes, footnotes (inline marker +
  trailing "Notes" section), links (visible text), images (alt placeholder).
- Tests: 6 integration tests that crack open the produced `.docx` and assert on
  `word/document.xml`, plus doc-tests. CI on Linux/macOS/Windows with `cargo fmt --check`,
  `cargo clippy -- -D warnings`, and `cargo test`.

### Known limitations (tracked follow-ups, not redesigns)
- No bibliography/citations (no stable Rust CSL processor yet).
- No math → OMML (no MathML→OMML Rust crate).
- Footnotes are inline markers, not real Word `footnotes.xml` parts.
- Lists use marker prefixes, not native Word numbering.
- Links/images render text/alt only (no hyperlink relation, no embedded media).
- No `--reference-doc` style inheritance and no PPTX — use the Python `md2star` for those.
