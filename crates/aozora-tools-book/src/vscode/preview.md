# Preview pane

`Aozora: Open Preview` opens a side panel WebView that renders the
active document as HTML. The preview updates on every text edit.

## Wire protocol

The WebView is **not** a separate LSP client. The extension owns one
`LanguageClient` instance and drives the WebView through the
`aozora/renderHtml` custom request:

1. The initial HTML returned by `aozora/renderHtml` when
   `Aozora: Open Preview` runs.
2. Refreshed HTML returned by `aozora/renderHtml` whenever
   `vscode.workspace.onDidChangeTextDocument` fires for the open
   document.

Inline-fold gaiji decorations on the source buffer use a separate
`aozora/gaijiSpans` request handled by `gaijiFold.ts`, but those
overlays live on the editor surface — the preview WebView itself
only consumes the `aozora/renderHtml` HTML.

## Refresh debouncing

The extension debounces `did_change` notifications client-side at
**150 ms** (`PREVIEW_RENDER_DEBOUNCE_MS` in `preview.ts`). A typing
burst collapses to a single `aozora/renderHtml` request at the tail
end of the burst, so the WebView is not flooded with intermediate
HTML diffs. The 150 ms matches the LSP server's
`PUBLISH_DEBOUNCE_MS` (the diagnostic-publish debounce in
`backend.rs`) so the preview frame and the diagnostic state align
within one debounce window.

## Writing mode (縦書き / 横書き)

Aozora Bunko works are vertically typeset in print, so the preview
defaults to **縦書き** (`writing-mode: vertical-rl`). Two ways to
change it:

- **Per-panel toggle** — `Aozora: プレビューの縦書き／横書きを切り替え`
  flips the active panel without touching the workspace setting.
  Useful when you want to glance at one document horizontally while
  keeping the default vertical for the rest.
- **Workspace default** — the `aozora.preview.writingMode` setting
  (`"vertical"` or `"horizontal"`). Changes propagate to every open
  preview immediately via `onDidChangeConfiguration`.

The vertical layout uses a fixed `height: calc(100vh - 3em)` and lets
the WebView's natural horizontal scrollbar handle overflow. Latin
characters and Arabic numerals stay horizontal inside vertical text
via `text-orientation: mixed`, matching the typesetting convention;
two-digit 縦中横 forms are already shipped by the renderer with the
appropriate class.

## CSS

The stylesheet lives inline inside `wrapHtml()` in `preview.ts`. It
intentionally pins a stable warm-tone background (`#fdf6e3`) and a
serif (`Hiragino Mincho ProN` / `Yu Mincho`) regardless of the active
editor theme — 青空文庫 prose reads best on a consistent typeset-book
surface. Theme-tracking is a follow-on if contributors prefer the
preview to match the editor's colour scheme; the trade-off is losing
that fixed reading background.

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
