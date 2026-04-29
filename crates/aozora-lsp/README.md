# aozora-lsp

Language Server (tower-lsp) for
[aozora-flavored markdown](https://github.com/P4suta/aozora)
(`.afm` / `.aozora` / `.aozora.txt`).

`aozora-lsp` speaks LSP on stdio. Pair it with the bundled
[VS Code extension](../../editors/vscode) or any LSP-capable
editor (Neovim, Helix, Emacs, Zed, …).

## Capabilities

| LSP method                       | Behaviour |
|----------------------------------|-----------|
| `textDocument/publishDiagnostics`| Every `aozora::Diagnostic` mapped to LSP `Diagnostic` (UTF-16 column-aware). |
| `textDocument/formatting`        | `parse ∘ serialize` (via `aozora_fmt::format_source`); single document-replace `TextEdit`. |
| `textDocument/hover`             | Cursor inside `※［＃…］` resolves the gaiji glyph + description via `aozora_encoding::gaiji`. |
| `textDocument/inlayHint`         | Resolved gaiji glyph rendered next to the source span. |
| `textDocument/linkedEditingRange`| Paired delimiters auto-rename together: `［` ↔ `］`, `《` ↔ `》`, `「` ↔ `」`, … |
| `textDocument/completion`        | Slug catalogue from `aozora::SLUGS`, parametric snippets, paired-container partner auto-insert. |
| `textDocument/foldingRange`      | Section / paragraph folding. |
| `textDocument/documentSymbol`    | Heading outline. |
| `textDocument/semanticTokens`    | Syntax-aware highlighting derived from the tree-sitter tree. |
| `workspace/executeCommand`       | `aozora.canonicalizeSlug` — canonicalise the slug under the cursor. |
| `aozora/renderHtml` *(custom)*   | Returns HTML for the active document. Drives the VS Code preview pane. |

## Architecture (one-liner)

Per-document state is split into a writer-side `Mutex<BufferState>`
(Rope buffer + tree-sitter incremental parse, segmented
paragraph-first per ADR-0008) and a reader-side
`ArcSwap<Snapshot>` for wait-free LSP request handling. The
semantic `aozora` parser is the source of truth for formatting,
diagnostics, and HTML rendering; the tree-sitter tree backs the
high-frequency syntactic queries (hover, inlay, completion,
codeAction).

ADRs that drove the current shape:

- [ADR-0001 — Shuttle randomized-schedule check](../../docs/adr/0001-shuttle-segment-cache.md)
- [ADR-0002 — LSP feature roadmap](../../docs/adr/0002-lsp-feature-roadmap.md)
- [ADR-0003 — Position encoding](../../docs/adr/0003-position-encoding.md)
- [ADR-0004 — Preview sync protocol](../../docs/adr/0004-preview-sync-protocol.md)
- [ADR-0005 — `ArcSwap` snapshot](../../docs/adr/0005-arcswap-snapshot.md)
- [ADR-0006 — `ropey::Rope` buffer](../../docs/adr/0006-rope-buffer.md)
- [ADR-0007 — Incremental gaiji-span rebuild](../../docs/adr/0007-incremental-gaiji-rebuild.md)
- [ADR-0008 — Paragraph-first document model](../../docs/adr/0008-paragraph-first-document-model.md)

## Run

```sh
# stdio LSP server (default)
cargo run --bin aozora-lsp

# Tracing on stderr (set RUST_LOG to your taste)
RUST_LOG=aozora_lsp=debug cargo run --bin aozora-lsp
```

## Test surface

```sh
cargo test  -p aozora-lsp --all-targets         # unit + integration
cargo test  -p aozora-lsp --features shuttle-tests   shuttle_doc_state   # randomized-schedule
cargo bench -p aozora-lsp --bench burst         # apply-edits p99 / max
```

The `tests/guardian.rs` 金庫番 suite is the panic-resistance,
idempotence, and concurrency invariant gate. The `burst` bench
(driven by `samples/bouten.afm`) is what the criterion-based
PR diff workflow (`.github/workflows/bench-diff.yml`) gates on.

Profiling pipeline (samply) is documented in
[`docs/profiling.md`](../../docs/profiling.md).

## Install

```sh
cargo install --git https://github.com/P4suta/aozora-tools --tag v0.1.3 --locked aozora-lsp
```

Or grab a pre-built binary from
[the releases page](https://github.com/P4suta/aozora-tools/releases) —
`aozora-lsp` is bundled in every `aozora-tools-vX.Y.Z-<target>`
archive, and the VS Code extension ships with the matching
binary baked in (no separate install needed).

## Repository

Part of the [aozora-tools](https://github.com/P4suta/aozora-tools)
workspace. See the [workspace README](../../README.md) for the
full picture.
