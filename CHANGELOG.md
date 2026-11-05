# Changelog

All notable changes to `md2star-rs` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

The foundation of the full port of md2star: one reader and one AST every backend
shares, the module layout the DOCX, PPTX and twin/PDF ports fill in, and the proof
that the core builds for iOS and Android.

### Added

- **A reader that keeps everything md2star's Markdown says.** `reader::read`
  returns a `Document`: YAML front matter (typed fields — title, subtitle,
  authors, date, lang, bibliography, csl, citation style, reference-section
  title, toc, keywords, `md2star_skip` — plus the whole map as `MetaValue`),
  Pandoc attributes `{#id .class key="v"}` on headings, code blocks, divs,
  images, links and table captions, automatic heading identifiers, images with
  source, alt, title and attributes, implicit figures, footnotes and inline
  notes `^[…]`, inline and display math with the raw LaTeX, math written in a
  code span (md2star's rule), citations `[see @key, p. 3; -@other]` and
  in-text `@key [p. 3]`, superscript and subscript (also inside a word:
  `2^10^`, `H~2~O`), strikeout, table alignments and `Table:` captions, task
  lists as item state, fenced divs `:::` (nested, with attributes), speaker
  notes `::: notes`, Mermaid blocks, raw HTML. The reader never fails;
  malformed input degrades to text and leaves a `Warning`.
- **`resources`**: resolve an image (relative path, absolute path, `file://`,
  `data:` URI, remote URL behind the new `net` feature and an explicit opt-in),
  identify it from its bytes (PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, SVG), read
  its pixel size without decoding it, and fit it to a box in EMU honouring
  `width=` / `height=`.
- **`convert(markdown, Format, &Options) -> Rendered`**: one entry point for
  every format, with warnings returned beside the bytes instead of printed.
- **The CLI tree of the Python md2star**: `md2star-rs docx | pptx | pdf | twin
  | audit | templates`, with its conversion flags (`--author`, `--bib`,
  `--bibliography-name`, `--citation-style`, `--lang`, `--date`,
  `--reference-doc`, `--toc`, `--offline`, `--allow-remote-images`, `-v`,
  `-q`; for decks `--template`, `--min-pt`, `--smart-layout`). `md2docx` and
  `md2pptx` are the same commands under their old names. A flag that is not
  honoured yet warns; a subcommand that is not ported yet exits with
  `not ported yet: …`.
- **Cargo features** `pdf`, `net`, `llm`, and `legacy-pptx` (default).
- **Shared fixtures** in `tests/fixtures/`: a document using every
  construction, a deck exercising layout hints, md2star's own fixtures and
  examples, PNG/JPEG/SVG images, two `.bib`, md2star's `.docx`/`.pptx`
  templates, and Pandoc-made reference outputs to compare against.
- **Cross-device proof in CI and in the pre-push hook**: `cargo check --lib
  --no-default-features` for `aarch64-apple-ios` and `aarch64-linux-android`.
- **BibTeX and citations, in pure Rust** (`md2star_rs::bib`): a BibTeX parser
  (`@string` macros, `#` concatenation, nested braces, `@comment`/`@preamble`,
  LaTeX accents and dashes decoded, names in the three BibTeX forms with their
  particles, `{Protected}` corporate names) and two built-in styles chosen by
  `--citation-style`, `citation-style:` or the *name* of a `csl:` file:
  Chicago author-date, Pandoc's default (`(Pearl 2009, 3; see also Pearl
  2000)`, `Pearl (2009, chap. 2)`, `(2000)`, `2009a`/`2009b`, `———.` for a
  repeated author), and IEEE numeric (`[1, p. 3]`, `[2], [1]`). An unknown key
  stays as written, in bold, with a warning. The reference list goes into a
  `::: {#refs}` div or at the end under `--bibliography-name`,
  `reference-section-title` or the localised "Bibliography"; `nocite` is read.
  CSL files themselves are not read.
- **`md2star-rs docx` is the equivalent of `md2star docx`, without Pandoc.** The
  DOCX writer now writes WordprocessingML itself, with Pandoc's vocabulary, so the
  same reference templates style both: a title block from the front matter
  (`Title`, `Subtitle`, `Author`, `Date`, `Abstract`, and `docProps/core.xml`),
  md2star's rule of the first heading as title; a table of contents (`--toc`,
  `toc: true`) as a Word field rebuilt on opening and pre-filled for readers that
  do not update fields; embedded images (PNG, JPEG, GIF, BMP, TIFF, and SVG with
  the PNG fallback Word requires, rasterised in process by `resvg` behind the new
  default feature `svg`) sized to the text column with their alt text, figures
  centred with an `ImageCaption`; real hyperlinks and bookmarks on headings,
  figures, tables and divs for `#id` links; native equations; citations and a
  `Bibliography`-styled reference list; Word footnotes everywhere, tables
  included; strikeout; `custom-style` divs; `---` as a page break; md2star's
  `MyTable` / `MyTableSmall` table styles. With `--reference-doc`, every part of
  the reference is kept (theme, settings, headers and footers with their
  images), its styles are used by id or by name and the missing ones added,
  its numbering kept with ours above it. The output stays byte-identical run
  after run.
- **`md2star-rs docx --csl FILE`**: the CSL file's name picks the citation style
  (the file is not read).
- **A LaTeX → OMML converter** (`docx::math`): fractions, scripts, roots, n-ary
  operators with bounds, limits under `\lim`/`\max`, `\left…\middle…\right`
  and matched brackets, the matrix environments (`pmatrix`, `bmatrix`, `cases`,
  `aligned`, `array`…), accents, over/under braces, `\mathbf`/`\mathbb`/
  `\mathcal`/`\text`/`\operatorname`, Greek letters, operators, relations and
  arrows, written as the same OMML structures Pandoc writes. An unknown command
  stays readable in the equation and raises a warning; nothing fails.
- **`twin`: DOCX and PPTX back to editable Markdown, as pure XML over the zip**
  (`md2star_rs::twin`, `md2star-rs twin`). From a `.docx`: the front matter
  (Title, Subtitle, Author, Date paragraphs and `docProps/core.xml`), headings
  by style name, `basedOn` chain or outline level, bold / italic / code /
  strikeout / superscript / subscript runs, bullet and numbered lists with
  their levels and start numbers from `numbering.xml`, pipe tables with
  alignment and merged cells, hyperlinks (relationships, anchors and
  `HYPERLINK` fields), footnotes and endnotes as `[^n]`, block quotes, code
  blocks, figure and table captions, task boxes, OMML equations back to LaTeX
  (`twin::omml_to_latex`: fractions, scripts, roots, n-ary operators,
  delimiters, matrices, accents, bars, functions, limits, braces; an unknown
  element keeps its text and warns). From a `.pptx`: the cover as front matter,
  one section per slide (`#` for section slides), bullets and numbered lists
  with levels, code, tables, pictures, two-column bodies, speaker notes as
  `::: notes`, `---` between slides, and Pandoc's footnote slide folded back
  into `[^n]`. Pictures are extracted to `<name>_media/` and linked relatively;
  `--no-images` leaves them out. Charts and SmartArt are reported, not
  invented. A PDF is refused with the reason (no OCR on every device). Tested
  by round trips Markdown → md2star-rs → twin → Markdown, against md2star's
  own `.docx`/`.pptx`, and LaTeX → OMML → LaTeX over the converter above.

### Changed

- The DOCX writer moved to `md2star_rs::docx` (`md2star_rs::writer` remains as
  a deprecated alias). Superscript and subscript are real vertical alignment,
  inline notes are real Word footnotes, and speaker notes stay out of the
  document. `docx::build` / `build_with_reference` (and the v0.4 functions built
  on them, `markdown_to_docx_bytes*`) keep the v0.4 `docx-rs` writer, since they
  return a `docx_rs::Docx`; `convert`, `docx::render` and the CLI use the new one.
- A table caption now precedes its table, in the `TableCaption` style, as Pandoc
  places it.
- A `.bib` that cannot be read or parsed is a warning, not a failed conversion:
  the citations it would have resolved stay as written.
- The `ppt-rs` writer moved behind the default feature `legacy-pptx`. ppt-rs
  pulls `zip` 0.6 with bzip2 and zstd compiled from C; that C is what kept the
  core from building for iOS and Android. Our own `zip` is now deflate-only.
- `Error` is `#[non_exhaustive]` and gains `Unsupported`, `Resource`, `Bib`,
  `Math`, `Pdf`, `Read` and `Llm`.

## [0.4.4] — 2026-10-19

### Fixed

- **Task-list state was parsed and then thrown away.** `ENABLE_TASKLISTS` is
  on, so `pulldown-cmark` hands the reader an `Event::TaskListMarker(bool)` for
  every `- [ ]` / `- [x]` item. The inline handler's catch-all discarded it,
  with a comment reading "Task-list markers, raw HTML, math, etc.: skipped in
  v0.1" — still there at 0.4.3.

  The result is that `- [ ] ship it` and `- [x] ship it` produced **identical**
  output. The checkbox is not decoration around the item's content; for a task
  list it *is* the content, and the document could no longer tell you what had
  been done.

  Items now carry a Unicode box, `☐` or `☑`. A box rather than a Word content
  control on purpose: this writer owns a plain OOXML run vocabulary, and the
  glyph survives copy-paste, PDF export and plain-text extraction, none of
  which a content control does.

  Both writers are fixed by the one change, since `pptx.rs` consumes the same
  `Block`/`Inline` AST as the DOCX path. Verified on both outputs.

### Checked, and found sound

- **Nothing else the reader sees is lost.** The same catch-all also drops raw
  HTML tags and math delimiters, which looked like the same bug. It is not:
  the text *inside* them arrives as its own `Event::Text` and is kept, so
  `<b>bold words</b>` keeps its words and `$E = mc^2$` survives as literal
  text. This writer renders no equations, but a reader of the document still
  sees what was written. Tests now pin that distinction so a future change to
  the catch-all cannot start swallowing content unnoticed.
- **The block-level catch-all was right all along**, and says so: unmodelled
  blocks are surfaced as a plain paragraph "rather than losing them".

## [0.4.3] — 2026-10-02

### Fixed
- **Bullet lists rendered as `1. 2. 3.` in Word.** `docx-rs` unconditionally writes its own
  `<w:abstractNum w:abstractNumId="1">` and `<w:num w:numId="1">` into `numbering.xml` ahead of
  anything we add, and our bullet instance was also `numId="1"`. The file therefore carried two
  instances with the same id, Word reads the first, and every bullet list picked up `docx-rs`'s
  default decimal numbering instead of our bullet glyphs. Our ids now start above the one
  `docx-rs` reserves, in both id namespaces. Two tests cover it: one asserts no id is declared
  twice, the other follows the paragraph → instance → definition chain Word follows and checks
  a bullet list lands on a `bullet` format.
- **An ordered list that does not start at 1 now renders from its own first number.**
  `5. five` opened at `1.` in the produced `.docx`. OOXML carries the first number on the
  abstract numbering definition's level, not on the per-list instance, so every list pointed at
  the one shared decimal definition and inherited its `<w:start w:val="1"/>`. A list starting
  anywhere other than 1 now gets a definition of its own; lists starting at 1 still share one,
  so `numbering.xml` does not grow on a document full of ordinary lists. Covered by three tests
  in `tests/convert.rs` that read `word/numbering.xml` directly.

### Changed
- **CI is one Linux job instead of a three-OS matrix.** The heavy, cross-platform testing is
  the local pre-push gate's job; CI confirms, it does not discover.
- **`rust-version` corrected from 1.74 to 1.88.** The declared floor had quietly stopped being
  true: `image`, `time` and `zip` in the dependency tree all require 1.88, so the crate fails
  to even resolve on anything older. Verified against a real 1.88.0 toolchain, and against
  1.85.0 to confirm the floor is where it says it is.
- The `ppt-rs = "=0.2.22"` pin stays, re-checked on 2026-10-02: upstream issue #11 is still
  open and 0.2.27 still fails to compile with `default-features = false`, verified against a
  scratch crate rather than assumed.

## [0.4.2] — 2026-09-05

### Changed
- Repo hygiene only, no change to the library or the binaries. Added
  `scripts/check-fresh-resolve.sh`, which builds the crate with no `Cargo.lock` — the way a
  downstream consumer resolves it — and wired it into CI (Linux) and a versioned `.githooks`
  pre-push gate. This is the check that would have caught the 0.4.0 breakage before publishing.

## [0.4.1] — 2026-09-05

### Fixed
- **`cargo install md2star-rs` failed on a clean machine.** The `ppt-rs` dependency was declared
  as `"0.2"`, and every release from 0.2.23 on fails to compile with `default-features = false`
  (`src/export/slide_render.rs` names `pdfrs` outside the feature gate that pulls it in). Our
  committed `Cargo.lock` pinned the working 0.2.22, so local builds and CI stayed green while any
  fresh consumer resolving the caret range got the newest patch and a build error. Pinned to
  `=0.2.22` until upstream fixes the gate.

## [0.4.0] — 2026-09-05

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
