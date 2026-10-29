# Landscape

[🇫🇷 PAYSAGE.md](https://github.com/warith-harchaoui/md2star-rs/blob/main/PAYSAGE.md) · 🇬🇧 English

Related and competing crates in the "Markdown to Office documents, in Rust" space, benchmarked against `md2star-rs`.
Ratings are ⭐ (1) to ⭐⭐⭐⭐⭐ (5), scored on `md2star-rs`'s intended job:
Markdown in, a faithful Office document out, from a single static binary with no Pandoc and no subprocess. A crate optimised for a very different job is not penalised; the score
just reflects fit to *this* niche.

## At a glance

<!-- TABLE:START -->
| Rust Markdown to Office | Markdown input | DOCX output | PPTX output | No external binary | Reference template / styles | Ready-to-run CLI | Deterministic output | PDF output |
| --- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **md2star-rs** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐ |
| docx-rs | ⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐ |
| rdocx | ⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| rpptx | ⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| pptxcraft | ⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐ |
| undoc | ⭐ | ⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ |
| Pandoc | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
<!-- TABLE:END -->

## Positioning map

<!-- FIGURE:START -->
2D representation of the table above.

![Positioning map](https://raw.githubusercontent.com/warith-harchaoui/md2star-rs/main/assets/landscape.png)

The map is a 2-D summary of the eight criteria, so read it as a shape, not a scoreboard. `md2star-rs` is at the top-right corner. The axes read **Horizontal: Independent ↔ Versatile** and **Vertical: Complete ↔ Consistent**.

The horizontal axis asks whether a project takes Markdown and ships as a usable tool — Markdown input, a reference template and a ready-to-run CLI are its three strongest loadings. The vertical axis trades determinism and self-containment against PDF output, which loads negatively.

Only two things sit on the right half: `md2star-rs` and Pandoc. Everything else on this map is a library you would have to drive yourself. That is the honest shape of the comparison — the real contest is against Pandoc, and `docx-rs`, `rpptx` and `pptxcraft` are components rather than rivals. This crate depends on the first two.

The vertical distance between `md2star-rs` at the top and Pandoc at the bottom is one trade, stated twice: Pandoc writes PDFs and has to be installed; this crate is a deterministic static binary and writes no PDF. `rdocx` is the row worth watching — closest to the centre horizontally, bottom half, because it is a library that nonetheless renders PDFs.
<!-- FIGURE:END -->

## Where the claims come from

Versions and publication dates read from the crates.io API on 29 October 2026:

| Crate | Version | Last published |
|---|---|---|
| `docx-rs` | 0.4.22 | 2026-07-21 |
| `rdocx` | 0.16.4 | 2026-10-28 |
| `rpptx` | 0.14.4 | 2026-10-28 |
| `pptxcraft` | 0.1.0 | 2026-10-21 |
| `undoc` | 0.27.1 | 2026-10-24 |

`md2star-rs`'s own ratings are from this repository at version 0.4.4.

- **Pandoc is in the table although it is not a Rust crate.** It is the thing this crate defines itself against — the Python `md2star` wraps it — so leaving it out would flatter the comparison. It scores 1 on `No external binary` for the same reason this crate exists, and 5 on `Ready-to-run CLI` because it is one — Pandoc's problem was never its interface.

- **The single 1 on this crate's row is PDF**, and it is the honest gap versus the Pandoc-backed original: `md2star-rs` writes `.docx` and `.pptx`, not `.pdf`. `rdocx` scores 5 there, and is the crate to reach for when a PDF is the deliverable.

- **`docx-rs` and `rpptx` score 1 on Markdown input** because neither reads Markdown at all — they are the OOXML writers. This crate depends on both rather than competing with them: the comparison is about what a caller gets end-to-end, not about library quality.

- **`undoc` runs the other way** — it extracts Markdown *out of* Office documents. It is in the table because it is the crate most often confused with this one in a search, not because it overlaps.

Deliberately left as opinion rather than sourced: the *pipeline ergonomics*
style columns, and the ordering of the rows below the reference.

## Ideas worth stealing

Researched rather than guessed: what each neighbour does better, and what this crate would have to change to match it. Nothing here is a commitment — it is the reading list the next version starts from.

- **`rdocx` — render a PDF from inside the crate** — This crate's single 1 is PDF output, and `rdocx` scores 5 there while remaining a Rust library: it renders pixel-identical PDFs and exports HTML and Markdown. The gap versus the Pandoc-backed Python `md2star` is therefore closable without reintroducing Pandoc, which was the whole reason this crate exists.

- **`undoc` — the reverse direction, Office back to Markdown** — The Python `md2star` has `twin`, which reads a finished document back into editable Markdown. The Rust crate has no reverse direction at all. `undoc` extracts DOCX, XLSX and PPTX to Markdown, plain text and JSON, and shows the shape such a reader would take.

- **`pptxcraft` — charts, tables, images and SVG in a deck** — The PPTX writer here turns each `#` heading into a slide with its bullets. `pptxcraft` emits charts, tables, images and SVG drawings, which is the next thing anyone generating a real deck asks for.

- **`rpptx` — read and re-render an existing deck** — Reading a deck back is what an audit or lint surface needs, and the Python side already has `md2star audit` for exactly that. `rpptx` can read, edit and render PowerPoint-compatible presentations.
