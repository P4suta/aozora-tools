# Changelog

All notable changes to aozora-tools are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/);
this project follows [Semantic Versioning 2.0.0](https://semver.org/),
with the 0.x major-zero contract: any **non-`-pre`** 0.x release may
include API breaks; aim for `0.MINOR.PATCH` reflecting `breaking.feature.fix`
once 1.0 ships.

## [Unreleased]

## [0.1.0] — 2026-04-28

Initial public release. The repository previously lived as a local
checkout (`repository = "local-only"` in `Cargo.toml`), iterated against
the sibling [`aozora`](https://github.com/P4suta/aozora) parser via
`[patch."file://..."]` overrides. v0.1.0 is the first build that:

- Pins `aozora` / `aozora-encoding` to the public **`v0.2.0`** tag
  via `git = "https://github.com/P4suta/aozora.git"`.
- Drops the `[patch."file://..."]` development override; reproducible
  builds across machines now work without a sibling `aozora` checkout.
- Ships GitHub Actions CI (`fmt --check`, `clippy --all-features`,
  `test --all-targets`, `doc --document-private-items`,
  `bench --no-run`, `typos`, `bun run check` for the VS Code
  extension) plus criterion baseline diffing on every PR
  (`bench-diff.yml`).

### Workspace contents

- `aozora-fmt` — idempotent CLI formatter built on
  `Document::parse ∘ AozoraTree::serialize`.
- `aozora-lsp` — Language Server Protocol implementation
  (tower-lsp). Publishes diagnostics, formatting, hover
  (gaiji glyph resolution), inlay hints, `linkedEditingRange`
  (paired delimiters), completion (slug catalogue),
  `aozora.canonicalizeSlug` code action, and the
  `aozora/renderHtml` custom request that drives the VS Code
  preview pane. Tree-sitter incremental parsing under the hood;
  the burst bench measures p99 / max apply-edits latency.
- `tree-sitter-aozora` — grammar consumed by `aozora-lsp` and
  any other tree-sitter-capable host.
- `aozora-tools-xtask` — repo automation (sanitizers harness,
  CPU-online introspection for bench scheduling).

### Editor integration

- `editors/vscode/` — VS Code extension client around
  `aozora-lsp`, plus the HTML preview pane (`Aozora: Open
  Preview`).

[Unreleased]: https://github.com/P4suta/aozora-tools/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/P4suta/aozora-tools/releases/tag/v0.1.0
