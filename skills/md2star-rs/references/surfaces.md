# Surfaces — md2star-rs

The Python suite's rule is five surfaces per helper: a library, two CLIs, a
FastAPI HTTP server, an MCP tool set mounted on that same app, and an
installable agent skill. This crate carries three.

## What exists

**Library** — `cargo add md2star-rs`. The primary surface. The `_bytes`
variants never touch the filesystem, which is what lets a server or a WASM
shell use the crate without a scratch directory.

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

**No GUI, no PDF, no document-to-Markdown twin mode.**

## Fidelity, not just language

This is the one crate in the suite where the Rust and Python implementations
differ in **output fidelity**, not only in ergonomics. Choosing wrongly
produces a document that is quietly missing something rather than an error.

| Need | Reach for |
|---|---|
| A single binary, nothing to install | `md2star-rs` |
| DOCX or PPTX bytes in memory | `md2star-rs` |
| Rust calling code | `md2star-rs` |
| Citations, a bibliography, `[@key]` | Python `md2star` |
| Math surviving into Word | Python `md2star` |
| Embedded images, hyperlink relations | Python `md2star` |
| Images or shapes in a deck | Python `md2star` |
| PDF output | Python `md2star` |
| Reading a PDF/DOCX/PPTX back into Markdown | Python `md2star` |
| HTTP API, MCP tools, GUI | Python `md2star` |
