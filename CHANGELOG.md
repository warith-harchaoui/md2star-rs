# Changelog

All notable changes to `md2star-rs` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [0.2.0] — unreleased

Native Word list numbering and real footnotes — the two highest-leverage upgrades over the
v0.1 placeholders, both verified against the produced OOXML.

### Added
- **Native Word list numbering.** Lists now emit a real `word/numbering.xml` (nine bullet
  levels + nine decimal levels) and reference it via `numPr`/`numId`/`ilvl` on each
  paragraph, instead of typing a `•`/`N.` glyph into the text. Each ordered list gets its
  own numbering instance so it **restarts at 1**; nesting maps to Word indent levels.
- **Real Word footnotes.** `[^x]` references become actual footnote references collected
  into `word/footnotes.xml` (via `Run::add_footnote_reference`), replacing the v0.1 inline
  marker + trailing "Notes" section.
- **Idempotence guarantee + test.** Footnote ids come from a per-document counter (not
  `docx-rs`'s process-global `Footnote::new` id) and no timestamps are written, so the same
  Markdown yields **byte-identical** `.docx` output every run. Covered by
  `conversion_is_idempotent`.

### Fixed
- Reader now captures **tight** list items / block quotes (CommonMark emits their text
  without a `<p>` wrapper); v0.1 silently dropped that inline content.

### Known limitations (still open)
- Footnote references inside **table cells** fall back to a text marker (`docx-rs` only
  collects footnotes from top-level document paragraphs).
- Custom ordered-list **start numbers** render from 1.
- Reference-doc style inheritance (`--reference-doc`), bibliography/citations, math→OMML,
  hyperlink relations, embedded images, and PPTX remain out of scope (see README).

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
