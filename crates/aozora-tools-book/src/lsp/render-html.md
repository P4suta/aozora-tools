# `aozora/renderHtml`

Returns rendered HTML for an open document.

## Request

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "aozora/renderHtml",
  "params": {
    "uri": "file:///path/to/doc.aozora",
    // Optional: pin to a specific document version. If omitted, the
    // server uses the latest published Snapshot.
    "version": 42
  }
}
```

## Response

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "html": "<article class=\"aozora-document\">...</article>",
    "version": 42,
    // Diagnostics that fired during the render. Same shape as a
    // standard LSP Diagnostic object so a client can deduplicate
    // against textDocument/publishDiagnostics if both pipelines
    // are running.
    "diagnostics": []
  }
}
```

## Errors

| Code | When |
|---|---|
| `-32602` (Invalid params) | `uri` is missing or not a known open document. |
| `-32603` (Internal error) | Render pipeline raised an unrecoverable error. |
| Custom `1001` | `version` was supplied but the server no longer holds a snapshot for that version (a newer change has superseded it). The client should re-send without the `version` pin or with the latest version it has seen. |

## When to use it

- **VS Code preview pane** — the canonical consumer. The extension
  registers a `Aozora: Open Preview` command that opens a WebView,
  sends `aozora/renderHtml` on every `did_change` notification it
  observes, and swaps the WebView's HTML with the response.
- **Static-export tools** — a `aozora-tools static-export` style
  tool can hit a one-shot `aozora-lsp` instance, send
  `did_open` + `aozora/renderHtml`, and write the result to disk.
- **Test harnesses** — golden-output tests for the renderer can
  drive `aozora-lsp` end-to-end via this method instead of building
  the render pipeline directly.

## CSS contract

The HTML is rendered with the canonical `aozora-*` class prefix
(`aozora-document`, `aozora-paragraph`, `aozora-ruby`,
`aozora-bouten`, …). The full class taxonomy is documented in the
sibling [`aozora`](https://github.com/P4suta/aozora) repository's
handbook. Clients are expected to ship their own stylesheet; the
LSP server does not embed CSS in the returned HTML.
