# Changelog

All notable changes to aozora-tools are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/);
this project follows [Semantic Versioning 2.0.0](https://semver.org/),
with the 0.x major-zero contract: any **non-`-pre`** 0.x release may
include API breaks; aim for `0.MINOR.PATCH` reflecting `breaking.feature.fix`
once 1.0 ships.

## [Unreleased]

## [0.1.3] — 2026-04-28

### Changed

- `aozora` / `aozora-encoding` pin → **`v0.2.3`**.
- Slimmed the GitHub Release binary matrix to three platforms:
  `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`. Intel macOS and `linux-musl` archives
  are no longer attached.

## [0.1.2] — 2026-04-28

### Changed

- `aozora` / `aozora-encoding` pin → **`v0.2.2`**.

### Fixed

- `release.yml` cross-builds now do an explicit `rustup target add`
  before invoking `cargo build`, so the `linux-musl` job stops
  failing on cold runners.

## [0.1.1] — 2026-04-28

### Added

- **`release.yml`** GitHub Actions workflow: tag pushes (`v*`) trigger
  cross-platform builds of `aozora-fmt` + `aozora-lsp`, attach
  archives + a `SHA256SUMS` manifest to the GitHub Release, and run
  `gh release edit --draft=false` once every artifact has uploaded.

### Changed

- `aozora` / `aozora-encoding` pin → **`v0.2.1`**.

## [0.1.0] — 2026-04-28

Initial public release.

### Workspace

- **`aozora-fmt`** — idempotent CLI formatter built on
  `Document::parse ∘ AozoraTree::serialize`.
- **`aozora-lsp`** — Language Server (tower-lsp). Diagnostics,
  formatting, gaiji hover, inlay hints, `linkedEditingRange` for
  paired delimiters, completion fed by the slug catalogue,
  `aozora.canonicalizeSlug` code action, and the
  `aozora/renderHtml` custom request that drives the VS Code
  preview pane. Tree-sitter incremental parsing under the hood;
  the burst bench measures p99 / max apply-edits latency.
- **`tree-sitter-aozora`** — grammar consumed by `aozora-lsp` and
  any other tree-sitter-capable host.
- **`aozora-tools-xtask`** — repo automation (sanitizers harness,
  CPU-online introspection for bench scheduling, samply pipeline).

### Editor integration

- **`editors/vscode/`** — VS Code extension client around
  `aozora-lsp`, plus the HTML preview pane (`Aozora: Open
  Preview`).

### CI

- GitHub Actions: `fmt --check`, `clippy --all-features`,
  `test --all-targets`, `doc --document-private-items`,
  `bench --no-run`, `typos`, `bun run check` for the VS Code
  extension. `bench-diff.yml` posts criterion baseline comparisons
  on every PR.

[Unreleased]: https://github.com/P4suta/aozora-tools/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/P4suta/aozora-tools/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/P4suta/aozora-tools/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/P4suta/aozora-tools/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/P4suta/aozora-tools/releases/tag/v0.1.0
