# Surfaces — md2star-rs

The Python suite's rule is five surfaces per helper: a library, two CLIs, a
FastAPI HTTP server, an MCP tool set mounted on that same app, and an
installable agent skill. This crate carries three.

## What exists

**Library** — `cargo add md2star-rs`. The primary surface, and the one an iOS or
Android app embeds (`default-features = false`). Every call takes and returns
bytes in memory, with its warnings beside the output; the `LayoutJudge` trait
takes plain strings, so a host app can implement it across an FFI boundary with
its own on-device model.

**Three CLIs** — `md2star-rs`, plus `md2docx` and `md2pptx`. The last two
predate this suite's "name the binary after the crate" convention and therefore
collide with the Python twin's console scripts. Rather than rename them and
break existing installs, `md2star-rs` was added beside them: it cannot collide,
it delegates to the same library calls, and its output is byte-for-byte
identical. `cli-reference.md` says which to reach for when.

**Agent skill** — this directory.

## What does not exist, and why

**No HTTP API.** The Python twin already serves one, over a strictly larger
feature set. A second with fewer routes would be another thing to keep in sync.

**No MCP.** The suite's rule is that MCP is *never* a second implementation:
`fastapi-mcp` mounts `/mcp` onto the FastAPI app the helper already serves, so
every MCP tool **is** one of its routes and the two cannot drift apart. Rust
has no equivalent of that mounting, so an MCP surface here would have to be a
separate implementation of the same tools.

**No GUI.** A cross-device library has no window of its own; the app that
embeds it has one.

## Fidelity, not just language

Since 0.5 the two implementations do the same work: DOCX with citations,
equations and images, decks on template layouts, PDF, twin and audit. What
remains different is listed here, because choosing wrongly produces a document
quietly missing something rather than an error.

| Need | Reach for |
|---|---|
| A single binary, nothing to install | `md2star-rs` |
| A phone or tablet app, no network | `md2star-rs` |
| Office or PDF bytes in memory, Rust calling code | `md2star-rs` |
| Byte-identical output run after run | `md2star-rs` |
| A full CSL style (APA, Vancouver… exactly) | Python `md2star` |
| Reading a PDF back into Markdown (OCR) | Python `md2star` |
| Cloning a template's example slides, a visual judge | Python `md2star` |
| HTTP API, MCP tools, GUI | Python `md2star` |
