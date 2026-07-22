# Changelog

All notable changes to `md2star-rs` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [0.4.0] — unreleased

A second output backend: **Markdown → PPTX**, reusing the same AST.

### Added
- **`md2pptx` — Markdown → PowerPoint (.pptx).** A new binary plus library API
  (`markdown_to_pptx_bytes` / `markdown_to_pptx_file` / `convert_path_to_pptx`). Each level-1
  heading (`#`) starts a slide titled after it; the body until the next `#` flattens to bullet
  points (ordered lists → numbered bullets, nested lists / deeper headings → sub-bullets, block
  quotes and code blocks → bullet lines, tables → one bullet per row). Content before the first
  `#` lands on an implicit first slide. Built on the `ppt-rs` crate (lean default feature set),
  which supplies the slide-master/layout/theme scaffolding.
- The backend reuses the existing reader/AST **unchanged** — the reader/writer seam paying off:
  a whole new format with zero reader edits.
- New `Error::Pptx` variant for presentation build/pack failures.

### Fixed / determinism
- **Byte-idempotent `.pptx` output.** `ppt-rs` stamps `SystemTime::now()` into
  `docProps/core.xml`, so two conversions a second apart (or concurrent threads crossing a second
  boundary) produced different bytes. The writer now re-packs the deck deterministically —
  pinning that timestamp to a fixed sentinel and stamping a fixed entry mtime — so identical
  Markdown yields identical bytes, even across threads. Guarded by
  `pptx_conversion_is_idempotent{,_under_concurrency}` (`tests/pptx.rs`).

## [0.3.0] — unreleased

Reference-doc styling — the last big gap versus the Pandoc-backed original.

### Added
- **`--reference-doc template.docx`** (Pandoc parity). The output inherits the template's
  styles, theme, fonts and page setup (size + margins, via the template's section
  properties); the template's body content is discarded. Headings and block quotes are
  emitted through the template's **named styles** when they exist, falling back to the inline
  formatting when they don't — so converting against a house template produces a document that
  looks like that template. Styles mapped: headings → `Heading1`…`Heading6`, block quotes →
  `Quote`, code blocks → `SourceCode` (else `HTMLPreformatted`), list items → `ListParagraph`.
  - Library API: `markdown_to_docx_bytes_with_reference(md, &template_bytes)` and
    `convert_path_with_reference(input, output, reference)`.
  - New `Error::Template` variant distinguishes "the reference isn't a valid `.docx`" from a
    plain I/O error.
  - Numbering ids we add for lists are offset above any the template already declares, so a
    template with its own numbering never collides with ours.
- **8 integration tests** (`tests/reference_doc.rs`) building templates in-memory: page-size
  inheritance, named heading/quote styling, graceful fallback when a style is absent, the
  Heading6 clamp, numbering non-collision, byte-idempotence on the reference path, and the
  invalid-template error.

## [0.2.1] — unreleased

Makes the v0.2.0 idempotence guarantee actually hold under concurrency — the fix that turns
CI green on macOS and Windows.

### Fixed
- **Byte-idempotence under parallel conversions.** `docx-rs` stamps every paragraph with a
  `w14:paraId` drawn from a **process-global atomic counter**. Because `cargo test` runs
  tests in parallel, sibling threads interleaved their id allocations between a test's two
  conversions, so identical Markdown produced *different* bytes — a flake that failed CI on
  macOS + Windows (ubuntu happened to schedule differently) while passing when the test ran
  alone. The writer now mints paragraph ids from its own **per-document counter** (mirroring
  how footnote ids were already handled), removing the dependency on global mutable state.
  Output is now byte-identical run-to-run *and* thread-to-thread.
- New regression test `conversion_is_idempotent_under_concurrency` hammers the conversion
  from 16 threads × 8 iterations and asserts a single distinct byte string, so the global
  counter can never silently creep back in.

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
  `docx-rs`'s process-global `Footnote::new` id) and no timestamps are written. Covered by
  `conversion_is_idempotent`. (This landed the footnote-id half; paragraph ids had the same
  process-global problem and were finished in 0.2.1 — the two together make output truly
  byte-identical, including under concurrency.)

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
