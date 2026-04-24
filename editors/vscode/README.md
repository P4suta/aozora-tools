# aozora — VS Code extension

Client for [aozora-lsp](../../crates/aozora-lsp).

## What you get

- **Syntax errors** — mismatched brackets, unclosed ruby, unknown
  `［＃…］` annotations surface as diagnostics in the Problems panel.
- **Formatter** — `Format Document` (Shift+Alt+F) rewrites the buffer
  into its canonical aozora form (`parse ∘ serialize`).
- **Gaiji hover** — hovering a `※［＃「...」、mencode］` reference
  shows the resolved Unicode character and the description.

## Setup

1. Build the LSP server once:

   ```bash
   cd ../..   # aozora-tools workspace root
   cargo build --release -p aozora-lsp
   ```

2. Point the extension at the binary. In VS Code settings:

   ```jsonc
   "aozora.lsp.path": "/absolute/path/to/aozora-tools/target/release/aozora-lsp"
   ```

   Or drop the binary into somewhere on `PATH` and leave the default
   (`aozora-lsp`).

3. Build and install the extension locally:

   ```bash
   bun install
   bun run compile
   # Press F5 from VS Code with this folder open to launch an
   # Extension Development Host for debugging, or
   # `bunx @vscode/vsce package` to build a .vsix for sideloading.
   ```

## Document association

Files with these extensions are treated as aozora:

- `.afm`
- `.aozora`
- `.aozora.txt`

Any other `.txt` file is untouched; set the language mode manually
(`Ctrl+K M` → "Aozora") to opt in.

## Local-only

This extension is not published to the Marketplace. It's a reference
client for `aozora-lsp`; any LSP-capable editor (Neovim, Helix, Emacs,
Zed) can drive the same server directly.
