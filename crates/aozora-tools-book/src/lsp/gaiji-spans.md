# `aozora/gaijiSpans`

Returns every gaiji (外字) reference in the document along with the
resolved Unicode character. Drives the VS Code extension's
inline-fold decorations (`※［＃...］` collapses to the resolved
glyph in-line) and is the underlying data source for the
[`aozora_lsp::inlay_hints`](https://p4suta.github.io/aozora-tools/aozora_lsp/inlay_hints/index.html)
library entry that generic LSP clients can use.

## Request

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "aozora/gaijiSpans",
  "params": {
    "uri": "file:///path/to/doc.aozora"
  }
}
```

## Response

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "version": 42,
    "spans": [
      {
        // LSP Range over the source text — the closing ］ inclusive.
        "range": {
          "start": { "line": 12, "character": 4 },
          "end":   { "line": 12, "character": 24 }
        },
        // Resolved Unicode codepoint as a hex string (no `U+` prefix).
        "codepoint": "29E3D",
        // The character itself, ready to render.
        "resolved": "𩸽",
        // The mencode the source asked for, e.g. "1-85-54".
        "mencode": "3-92-54",
        // True iff the resolution went through the encoder's
        // fallback chain (PUA passthrough or "no mapping found").
        // Editors typically render fallback resolutions in a
        // muted style.
        "fallback": false
      }
    ]
  }
}
```

## Errors

Same shape as [`aozora/renderHtml`](render-html.md); see that page
for the codes.

## Performance

The data is computed off the latest `Snapshot` (no extra parse). On
a 6 MB document the response wire size is dominated by the spans
list, not the per-span payload — clients should consume spans
incrementally if they support it. The VS Code extension consumes
the whole array in one pass and uses the LSP `Range` to drive
`vscode.window.createTextEditorDecorationType`.

## When to use the library entry instead

Generic LSP clients that use `inlay_hints` already do something
similar: the `aozora_lsp::inlay_hints::compute` function returns
the same data shaped as `lsp_types::InlayHint` so it lands directly
in `textDocument/inlayHint` responses. Use the custom request when
the editor wants the raw span data (e.g. for non-inlay decorations
like inline-fold). Use inlay hints when the editor's standard inlay
surface is the right rendering channel.
