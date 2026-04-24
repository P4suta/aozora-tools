# aozora-tools

Authoring support for [aozora-flavored-markdown](../afm).

The parser / AST / lexer / encoding crates live in the sibling `afm`
repository; this repository hosts the editor-surface tooling that
consumes them. See `afm/docs/adr/0009-authoring-tools-live-in-sibling-repositories.md`
for the split rationale.

## Crates

| Crate | Purpose |
|---|---|
| `crates/aozora-fmt` | `aozora-fmt` CLI — idempotent formatter built on `parse ∘ serialize`. |
| `crates/aozora-lsp` | `aozora-lsp` — Language Server Protocol implementation (tower-lsp). Publishes diagnostics, serves `textDocument/formatting` and `textDocument/hover` (gaiji resolution). |

## Editor integrations

| Path | Editor |
|---|---|
| `editors/vscode/` | VS Code extension — launches `aozora-lsp` as a subprocess. |

Any LSP-capable editor (Neovim, Helix, Emacs, Zed, …) can use
`aozora-lsp` directly; the VS Code extension is the one packaged
reference client.

## Local development

This tree depends on the sibling `afm` checkout at `../afm` via
`path` dependencies. If your `afm` checkout lives elsewhere, adjust
the `path = "../afm/..."` entries in `Cargo.toml` accordingly.

```bash
cargo build --workspace
cargo test  --workspace
cargo run   --bin aozora-fmt -- sample.txt
cargo run   --bin aozora-lsp                   # speaks LSP on stdio

# VS Code extension
cd editors/vscode
bun install
bun run compile
```

## Distribution

Local-only. No publishing target (crates.io, VS Code Marketplace,
npm) is configured; that decision is deferred until the tools are
battle-tested.
