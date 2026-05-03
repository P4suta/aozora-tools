# aozora-tools

`aozora-tools` is the editor-surface tooling for
[青空文庫記法 (Aozora Bunko notation)](https://github.com/P4suta/aozora):

- **`aozora-fmt`** — idempotent CLI formatter (`rustfmt --check` / `--write` ergonomics)
- **`aozora-lsp`** — Language Server Protocol implementation (`tower-lsp`, stdio)
- **`tree-sitter-aozora`** — tree-sitter grammar driving incremental reparses inside `aozora-lsp`
- **VS Code extension** — bundles `aozora-lsp` and adds an HTML preview pane

The parser, AST, lexer, encoding and renderer live in the sibling
[`aozora`](https://github.com/P4suta/aozora) repository on a strict
release cadence with corpus sweeps; this repository iterates on the
editor surface (LSP capabilities, VS Code UX, preview WebView, the
tree-sitter grammar). The split keeps the parser tag stable while the
authoring tools move quickly.

## Who this handbook is for

- **Authors** writing `.aozora` / `.afm` documents — see [Install](getting-started/install.md)
  and the per-editor quickstart pages.
- **Editor integrators** wiring a non-VS-Code client to `aozora-lsp` —
  start with [Standard LSP capabilities](lsp/capabilities.md), then
  [Custom protocol extensions](lsp/extensions.md) for the
  `aozora/renderHtml` and `aozora/gaijiSpans` requests.
- **Contributors** working on the tooling itself — jump to
  [Development loop](contrib/dev.md).

## Two-tier docs

- **This handbook** is the user-facing prose: protocol contracts,
  architecture, contributing flow.
- The **rustdoc API reference** ([/api/](./api/)) is the
  symbol-level navigator for `aozora-fmt`, `aozora-lsp`, and
  `aozora-tools-xtask`.

Both are deployed from the same `docs.yml` GitHub Pages workflow.
