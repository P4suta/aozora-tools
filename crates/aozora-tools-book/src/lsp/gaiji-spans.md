# `aozora/gaijiSpans`

Returns every gaiji (外字) reference in the document along with the
resolved Unicode character. Drives the VS Code extension's
inline-fold decorations (`※［＃...］` collapses to the resolved
glyph in-line). Any LSP client can consume this request directly for
the same data.

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

The data is computed off the latest `DocSnapshot` (no extra parse).
Response wire size scales with the span count, so clients that can
should consume spans incrementally. The VS Code extension consumes
the whole array in one pass and uses the LSP `Range` to drive
`vscode.window.createTextEditorDecorationType`.

## Relationship to `textDocument/inlayHint`

The server deliberately does **not** advertise `textDocument/inlayHint`:
the VS Code extension renders the resolved glyph through
`gaijiSpans`-driven decorations, and a parallel inlay layer duplicated
the visual. Clients that want the resolved-gaiji data consume
`aozora/gaijiSpans` directly and shape it however they like — inlay
hints, inline folds, hovers, etc.
