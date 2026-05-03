# Overview

`aozora-lsp` is a `tower-lsp` server that speaks LSP over stdio. It
runs as a long-lived subprocess of the editor and answers requests
about open documents.

## Workspace position

The server is the load-bearing piece of the editor surface: every
client (VS Code extension, Neovim, Helix, Emacs, Zed) hits the same
binary, so capability changes here propagate everywhere without
per-editor work. Editor-specific concerns (preview WebView in VS
Code, syntax highlighting in tree-sitter clients) build *on top* of
the LSP surface using the [custom protocol extensions](extensions.md);
generic LSP clients still get every standard capability.

## Architectural pillars

Three decisions shape the rest of this section:

1. **Snapshot reads are wait-free.** Each open document holds a
   read-side `Snapshot` behind an `ArcSwap`; every request handler
   resolves the document via a single atomic load. No request stalls
   on a parse, even mid-keystroke.
2. **Writes go through one mutex per document.** The editor-driven
   `did_change` path acquires a `parking_lot::Mutex` around a
   `BufferState` (the rope buffer + tree-sitter state), applies the
   text edits, and atomically swaps a fresh `Snapshot` into the
   `ArcSwap`. Readers see the new snapshot on their next load.
3. **The semantic re-parse is debounced and runs off the request
   thread.** A 200 ms debounce coalesces keystroke bursts; the
   re-parse runs inside `tokio::task::spawn_blocking` so the async
   runtime stays responsive to hover / inlay / codeAction requests
   that arrive during the debounce window.

[State model](state-model.md) walks through the data structures and
the lock graph in detail.

## Capability surface

- **Standard LSP** — diagnostics, formatting, hover, completion,
  linked editing, folding, document symbols, semantic tokens. See
  [Standard LSP capabilities](capabilities.md) for the full list.
- **Workspace command** — `aozora.canonicalizeSlug` rewrites the
  slug at the cursor to its canonical form. See
  [Workspace commands](commands.md).
- **Custom requests** — `aozora/renderHtml` returns rendered HTML
  for the open document; `aozora/gaijiSpans` returns every
  resolvable gaiji span. See [Custom protocol extensions](extensions.md).

## Position encoding

`aozora-lsp` advertises both `utf-16` (LSP default) and `utf-8`
position encodings during initialise. Clients that opt into `utf-8`
(VS Code 1.74+, Neovim 0.10+, Helix, Zed) skip the per-edit codepoint
conversion that LSP requires for `utf-16`; on a 6 MB document this
is the difference between a < 1 ms and a ~5 ms `did_change` path.
The buffer is held as a UTF-8 rope ([`ropey`](https://docs.rs/ropey)),
so `utf-8` is the cheaper encoding internally.
