# Preview pane

`Aozora: Open Preview` opens a side panel WebView that renders the
active document as HTML. The preview updates on every text edit.

## Wire protocol

The WebView is **not** a separate LSP client. The extension owns one
`LanguageClient` instance and forwards three things to the WebView:

1. The initial HTML returned by `aozora/renderHtml` on open.
2. Refreshed HTML returned by `aozora/renderHtml` on every
   `did_change` notification the extension observes.
3. Optional gaiji-span overlays returned by `aozora/gaijiSpans`,
   for cases where the WebView wants to highlight unresolved gaiji
   in the rendered output.

The WebView itself runs in a sandboxed iframe and posts messages to
the extension host via `acquireVsCodeApi()`. Messages are typed and
schema-validated at the extension boundary.

## Refresh debouncing

Re-rendering on every keystroke would flood the WebView with HTML
diffs the user cannot read. The extension piggybacks on the LSP
server's 200 ms debounce: it listens for the
`textDocument/publishDiagnostics` push (which only fires after a
debounced re-parse) and triggers a single `aozora/renderHtml` call
in response. The result: the preview lags typing by ~200 ms, but
each rendered frame corresponds to a real semantic state.

## CSS

The bundled extension ships a small stylesheet
(`media/preview.css`) that resolves the canonical `aozora-*` class
prefix into typography defaults — vertical-line height, ruby font
sizing, gaiji fallback box, container indentation. The stylesheet
is theme-aware: it consumes VS Code's CSS custom properties
(`--vscode-editor-foreground`, …) so the preview tracks the active
editor theme without per-theme code.

## Limitations

- **Print / export** — the WebView is not exportable to PDF or
  static HTML through VS Code's API. For static export, run
  `aozora-lsp` standalone, send `did_open` + `aozora/renderHtml`,
  and write the response to disk.
- **Cross-document navigation** — the preview shows one document at
  a time. Container navigation (jumping to the matching close) is a
  source-side LSP concern (folding ranges + go-to-definition), not a
  preview concern.
- **Editing** — the preview is read-only. Edits go through the
  source editor; the WebView mirrors them.
