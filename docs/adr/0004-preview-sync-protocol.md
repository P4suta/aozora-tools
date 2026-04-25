# ADR-0004 — Preview sync protocol (`aozora/renderHtml`)

Status: accepted (2026-04)

## Context

Phase 3.1 of the editor-integration sprint shipped a Markdown-Preview-
style WebView pane in the VS Code extension. The pane needs the
HTML rendering of the active document and must stay in lock-step
with editor edits. Two design choices presented themselves:

1. **Embed `aozora-wasm`** in the WebView and render client-side.
2. **Add an LSP custom request** the extension calls into the
   already-running `aozora-lsp` daemon.

## Decision

Pick **option 2**. The custom request is named `aozora/renderHtml`
and lives in the `aozora-lsp` crate (`backend.rs`).

### Wire format

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "aozora/renderHtml",
  "params": { "uri": "file:///path/to/doc.aozora" }
}
```

Response (success):

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": { "html": "<p>…</p>" }
}
```

Response (no document at URI):

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "error": {
    "code": -32602,
    "message": "no document at uri"
  }
}
```

### Why custom over standard LSP

LSP has no `textDocument/render` request. The `aozora/`-prefixed
custom request is the documented escape hatch; tower-lsp wires it
via `LspService::build(...).custom_method(...)`. Naming it
`aozora/...` keeps the namespace clear: any client that does not
recognise the method falls through to the editor's default
behaviour.

### Why one request per render, not a notification stream

Pull beats push for this use case:

- The webview decides when it needs a fresh render (debounce on
  `onDidChangeTextDocument` in `preview.ts`).
- `aozora/renderHtml` is idempotent — repeated calls with the same
  URI yield the same HTML (modulo edits). Caching is the client's
  prerogative.
- No subscription bookkeeping, no leaked listeners on
  `did_close`.

### Cursor sync (planned, deferred)

The current implementation renders on edit. Bidirectional cursor
sync (editor → webview scroll, webview click → editor reveal) is
the obvious next step:

- Editor → webview: emit `<span data-src-line="...">` per paragraph
  in the LSP renderer; the webview's `postMessage({ line })` handler
  scrolls to it.
- Webview → editor: the webview posts `{ type: "reveal", line }` via
  `acquireVsCodeApi().postMessage`; the extension calls
  `editor.revealRange`.

Both legs ride the same `aozora/renderHtml` payload — the renderer
just needs to add `data-src-line` anchors. Captured here so a future
contributor lands cursor sync as a small follow-on instead of a
new architecture.

## Consequences

- VS Code extension carries `editors/vscode/src/preview.ts` with the
  `aozora.openPreview` command, a `WebviewPanel` per document URI,
  and a 150 ms debounce on `did_change` re-render.
- `aozora-lsp` carries the `Backend::render_html` method and the
  `RenderHtmlParams` / `RenderHtmlResult` structs at the top of
  `backend.rs`.
- The same custom request is reachable from any LSP client that
  speaks `aozora/renderHtml`; the VS Code extension is the
  reference consumer.
- WASM remains available for `aozora-wasm` consumers (e.g. a
  static-site preview page) without touching the LSP path.
