# aozora-tools

Authoring support for [aozora-bunko notation](https://github.com/P4suta/aozora) — the formatter, LSP server, tree-sitter grammar, and VS Code extension.

<p align="center">
  <a href="https://github.com/P4suta/aozora-tools/actions/workflows/ci.yml"><img alt="ci" src="https://github.com/P4suta/aozora-tools/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/P4suta/aozora-tools/actions/workflows/docs.yml"><img alt="docs deploy" src="https://github.com/P4suta/aozora-tools/actions/workflows/docs.yml/badge.svg"></a>
  <a href="https://github.com/P4suta/aozora-tools/releases/latest"><img alt="latest release" src="https://img.shields.io/github/v/release/P4suta/aozora-tools?display_name=tag&sort=semver"></a>
  <a href="./LICENSE-APACHE"><img alt="license" src="https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue"></a>
  <a href="./rust-toolchain.toml"><img alt="msrv" src="https://img.shields.io/badge/rust-1.95%2B-orange"></a>
</p>

<p align="center">
  📖 <a href="https://p4suta.github.io/aozora-tools/"><strong>Handbook</strong></a>
  · 🦀 <a href="https://p4suta.github.io/aozora-tools/api/"><strong>API reference (rustdoc)</strong></a>
  · 📦 <a href="https://github.com/P4suta/aozora-tools/releases"><strong>Releases &amp; binaries</strong></a>
  · 📝 <a href="./CHANGELOG.md"><strong>Changelog</strong></a>
</p>

The parser / AST / lexer / encoding crates live in the sibling
[`aozora`](https://github.com/P4suta/aozora) repository; this
repository hosts the editor-surface tooling that consumes them.
The parser is a long-lived correctness artefact released on a
strict cadence with corpus sweeps; the editor surface here moves
faster (LSP capabilities, VS Code UX, preview WebView, tree-sitter
grammar). Splitting the repos keeps the parser tag stable while
this repo iterates.

## Crates

| Crate | Purpose |
|---|---|
| [`crates/aozora-fmt`](./crates/aozora-fmt) | `aozora-fmt` CLI — idempotent formatter built on `Document::parse ∘ AozoraTree::serialize`. |
| [`crates/aozora-lsp`](./crates/aozora-lsp) | `aozora-lsp` — Language Server Protocol implementation (tower-lsp). Publishes diagnostics, formatting, hover (gaiji), `linkedEditingRange` (paired delimiters), completion (slug catalogue), folding ranges, document symbols, semantic tokens, `aozora.canonicalizeSlug` workspace command, plus the `aozora/renderHtml` and `aozora/gaijiSpans` custom requests the VS Code extension consumes. |
| [`crates/tree-sitter-aozora`](./crates/tree-sitter-aozora) | Tree-sitter grammar for incremental parsing inside `aozora-lsp`; usable from any tree-sitter host. |
| [`crates/aozora-tools-xtask`](./crates/aozora-tools-xtask) | Repo automation (sanitizers harness, CPU-online introspection for bench scheduling). |

## Editor integrations

| Path | Editor |
|---|---|
| [`editors/vscode/`](./editors/vscode) | VS Code extension — launches `aozora-lsp` as a subprocess and adds the HTML preview pane (`Aozora: Open Preview`). |

Any LSP-capable editor (Neovim, Helix, Emacs, Zed, …) can use
`aozora-lsp` directly; the VS Code extension is the one packaged
reference client. The preview pane is a VS Code-only WebView; other
editors get every other capability through the standard LSP surface.

## LSP capabilities

`aozora-lsp` advertises:

- `textDocument/publishDiagnostics`
- `textDocument/formatting`
- `textDocument/hover` (gaiji resolution)
- `textDocument/linkedEditingRange` (`［` ↔ `］`, `《` ↔ `》`, `「` ↔ `」`, …)
- `textDocument/completion` (slug catalogue from `aozora::SLUGS`,
  parametric snippets, paired-container partner auto-insert)
- `textDocument/foldingRange`, `textDocument/documentSymbol`,
  `textDocument/semanticTokens`
- `workspace/executeCommand` → `aozora.canonicalizeSlug`
- Custom requests:
  - `aozora/renderHtml` — VS Code preview WebView consumes this.
  - `aozora/gaijiSpans` — every resolvable gaiji span in the
    document; the VS Code extension uses it to drive inline-fold
    decorations (resolved glyph next to `※［＃…］`). Generic LSP
    clients can opt into the same data via the
    `aozora_lsp::inlay_hints` library entry instead.

See the [handbook](https://p4suta.github.io/aozora-tools/) for the
LSP capability surface, custom protocol extensions, and the
preview wire format.

## Build and run

The workspace pins [`aozora`](https://github.com/P4suta/aozora) at the
public `v0.2.3` tag, so a fresh clone needs nothing more than a
matching Rust toolchain (1.95.0, see `rust-toolchain.toml`) and
[`bun`](https://bun.sh/) for the VS Code extension.

```sh
# Native cargo build (host toolchain — no Docker, by design).
cargo build --workspace
cargo test  --workspace --all-targets
cargo run   --bin aozora-fmt -- sample.txt
cargo run   --bin aozora-lsp                   # speaks LSP on stdio

# VS Code extension
cd editors/vscode
bun install --frozen-lockfile
bun run compile
# Then F5 in VS Code with this folder open → Extension Development Host.
```

The pre-push hook (`lefthook install`) runs `fmt --check`, clippy
(`-D warnings`), the full test suite, `bench --no-run`, doc build
(`-D warnings`), `typos`, and `bun run check` for the VS Code extension
before any push lands.

## Install

Pre-built `aozora-fmt` + `aozora-lsp` binaries for **Linux x86_64**,
**macOS arm64**, and **Windows x86_64** are attached to every GitHub
Release — see [the releases page](https://github.com/P4suta/aozora-tools/releases)
and pick a `aozora-tools-vX.Y.Z-<target>.{tar.gz,zip}`. SHA256 sums
are published as `SHA256SUMS` next to the archives.

Or build from source:

```sh
cargo install --git https://github.com/P4suta/aozora-tools --tag v0.1.3 --locked aozora-fmt
cargo install --git https://github.com/P4suta/aozora-tools --tag v0.1.3 --locked aozora-lsp
```

## Versioning and release

- `aozora-tools` follows [SemVer 2.0.0](https://semver.org/) with the
  0.x major-zero contract (any `0.MINOR` bump may break API).
- `aozora` parser pin is updated as a deliberate workspace bump, never
  silently — it shows up in `Cargo.toml` as a one-line `tag = "..."`
  diff plus the corresponding `CHANGELOG.md` entry.
- The VS Code extension (`editors/vscode/package.json`) is versioned
  independently on its Marketplace cadence; it bundles whatever
  `aozora-lsp` build is current at release time.

See [`CHANGELOG.md`](./CHANGELOG.md) for what shipped when.

## License

Dual-licensed under
[Apache 2.0](./LICENSE-APACHE) **OR**
[MIT](./LICENSE-MIT) at your option, matching the upstream `aozora`
parser. Contributions land under the same dual licence by default
(see [`CONTRIBUTING.md`](./CONTRIBUTING.md)).
