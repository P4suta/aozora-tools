# aozora-tools

Authoring support for [aozora-bunko notation](https://github.com/P4suta/aozora).

The parser / AST / lexer / encoding crates live in the sibling
`aozora` repository; this repository hosts the editor-surface tooling
that consumes them. See `aozora/docs/adr/0009-authoring-tools-live-in-sibling-repositories.md`
for the split rationale.

## Crates

| Crate | Purpose |
|---|---|
| `crates/aozora-fmt` | `aozora-fmt` CLI — idempotent formatter built on `Document::parse ∘ AozoraTree::serialize`. |
| `crates/aozora-lsp` | `aozora-lsp` — Language Server Protocol implementation (tower-lsp). Publishes diagnostics, formatting, hover (gaiji), inlay hints (gaiji glyph), `linkedEditingRange` (paired delimiters), completion (slug catalogue), `aozora.canonicalizeSlug` code action, and the `aozora/renderHtml` custom request that powers the VS Code preview pane. |

## Editor integrations

| Path | Editor |
|---|---|
| `editors/vscode/` | VS Code extension — launches `aozora-lsp` as a subprocess and adds the HTML preview pane (`Aozora: Open Preview`). |

Any LSP-capable editor (Neovim, Helix, Emacs, Zed, …) can use
`aozora-lsp` directly; the VS Code extension is the one packaged
reference client. The preview pane is a VS Code-only WebView; other
editors get every other capability through the standard LSP surface.

## Local development

This tree depends on the sibling `aozora` checkout via `git +
[patch."file://…"]` overrides in the workspace `Cargo.toml`. The
defaults assume `~/projects/aozora`; if your checkout lives elsewhere,
adjust the `[patch."file:///home/yasunobu/projects/aozora"]` block.

```bash
cargo build --workspace
cargo test  --workspace
cargo run   --bin aozora-fmt -- sample.txt
cargo run   --bin aozora-lsp                   # speaks LSP on stdio

# VS Code extension
cd editors/vscode
bun install
bun run compile
# Then F5 in VS Code with this folder open → Extension Development Host.
```

## LSP capabilities

After Phase 0 (`aozora-tools` migration to the 0.2 `aozora` crate)
plus Phase 2 of the editor-integration sprint, `aozora-lsp` advertises:

- `textDocument/publishDiagnostics`
- `textDocument/formatting`
- `textDocument/hover` (gaiji resolution)
- `textDocument/inlayHint` (resolved gaiji glyph next to `※［＃…］`)
- `textDocument/linkedEditingRange` (`［` ↔ `]`, `《` ↔ `》`, `「` ↔ `」`, …)
- `textDocument/completion` (slug catalogue from `aozora::SLUGS`,
  parametric snippets, paired-container partner auto-insert)
- `workspace/executeCommand` → `aozora.canonicalizeSlug`
- Custom: `aozora/renderHtml` — VS Code preview WebView consumes this.

See `docs/adr/0001-lsp-feature-roadmap.md` for the full roadmap and
`docs/adr/0003-preview-sync-protocol.md` for the preview wire format.

## Distribution

Local-only. No publishing target (crates.io, VS Code Marketplace,
npm) is configured; that decision is deferred until the tools are
battle-tested.
