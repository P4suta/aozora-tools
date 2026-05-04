# Changelog

All notable changes to aozora-tools are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/);
this project follows [Semantic Versioning 2.0.0](https://semver.org/),
with the 0.x major-zero contract: any **non-`-pre`** 0.x release may
include API breaks; aim for `0.MINOR.PATCH` reflecting `breaking.feature.fix`
once 1.0 ships.

## [Unreleased]

### Added

- **CI `coverage` job** (`.github/workflows/ci.yml`) runs
  `cargo llvm-cov nextest --workspace --all-features` and hard-gates
  on **line ≥ 80% / region ≥ 70%**. Region coverage is the stable-
  toolchain analogue of `--fail-under-branches` (which is nightly-
  only): LLVM emits one MC counter per `if` / `match` arm, so each
  branch is counted independently. Publishes a job summary, uploads
  lcov + HTML as a 14-day artefact.
- **`xtask coverage`** subcommand wrapping the same flag set so
  local + CI numbers stay comparable. The `IGNORE_FILENAME_REGEX`
  (xtask sources + binary `main.rs` entry points excluded from the
  denominator) is centralised in
  `crates/aozora-tools-xtask/src/coverage.rs` and reused by the
  workflow, so CI and local runs cannot drift.
- 11 invariant tests in `crates/aozora-lsp/src/incremental.rs`
  pinning the tree-sitter incremental contract: 1-shot parse ≡
  initial parse + `apply_edit`, the Rope-driven parse paths, and
  `chunk_callback` boundaries. Region coverage on that module:
  70.81% → 99.61%.
- **VS Code preview writing-mode toggle** —
  `Aozora: プレビューの縦書き／横書きを切り替え` command plus the
  `aozora.preview.writingMode` setting (`"vertical"` /
  `"horizontal"`, default `"vertical"`). Implemented as a
  `writing-mode: vertical-rl` overlay on the inline preview CSS
  in `editors/vscode/src/preview.ts`. Aozora Bunko works are
  vertically typeset in print, so the preview matches that
  orientation by default.
- CI `rust` job's `cargo nextest` step now passes
  `--features aozora-lsp/shuttle-tests`, so the Shuttle randomized-
  schedule concurrency checker runs on every PR (1,000 iterations
  by default; the nightly cron raises `AOZORA_SHUTTLE_ITERS`).
  Previously the checker only fired in the `coverage` job (which
  uses `--all-features`), so a coverage-side regression could mask
  a shuttle failure.
- **CI `msrv` job** — `cargo check --workspace --all-features
  --all-targets --locked` against the declared
  `rust-version = "1.95.0"`. Catches MSRV regressions that the
  canonical-toolchain `rust` job would miss.

### Changed

- handbook `lsp/state-model.md` rewritten to match the
  paragraph-first refactor in `crate::state` and `crate::paragraph`:
  `Vec<MutParagraph>` per-paragraph trees, snapshot reuse via
  `Arc::clone` and `ParagraphSnapshot::shifted_to`, the actual
  150 ms `PUBLISH_DEBOUNCE_MS` (previously documented as 200 ms),
  the correct `shuttle-tests` feature name, and the 6 MB /
  ~33 ns/byte parse-cost numbers behind the design.
- handbook `vscode/preview.md` rewritten to match `preview.ts`:
  150 ms client-side debounce (previous text described
  rebroadcasting `publishDiagnostics` at 200 ms, which is not what
  the extension does), CSS lives inline in `wrapHtml()` (no
  separate `media/preview.css`), new writing-mode section.
- handbook `lsp/extensions.md`: the CHANGELOG cross-reference now
  points to the actual Keep a Changelog sections (`Changed` /
  `Removed`) instead of a non-existent "Custom LSP protocol"
  section.
- `crates/aozora-lsp/src/incremental.rs` module docstring
  repositioned: `IncrementalDoc` is now described as the
  measurement control for `benches/burst.rs`, since production
  uses the per-paragraph trees in
  `crate::paragraph::MutParagraph`. The `input_edit` helper
  continues to be reused by `BufferState`.
- `crates/aozora-lsp/src/backend.rs`: dropped the `Stage 5` /
  `Stage 7` development-phase markers from doc comments,
  replacing them with descriptions of what the code does now.
- `crates/aozora-tools-xtask/src/main.rs` docstring no longer
  claims the wrapper passes `--branch` (which is nightly-only);
  notes that region coverage is the stable analogue.
- `.github/workflows/ci.yml` coverage-job comments no longer
  reference `Phase B` / `continue-on-error: true` — that hard-gate
  rollout already happened, and `continue-on-error` was never
  introduced into the workflow.
- `editors/vscode/README.md` and the preview walkthrough
  (`media/walkthrough/04-preview.md`) document the writing-mode
  toggle and the new `aozora.preview.writingMode` setting.

### Fixed

- The `coverage` CI job has been silently failing on every main
  push since it landed in #12 with `error: failed to generate
  report: failed to create file 'target/llvm-cov/lcov.info': No
  such file or directory`. The 292 instrumented tests passed, but
  `cargo llvm-cov report --lcov --output-path …/lcov.info` does
  not auto-create the parent directory, so the report (and the
  gate that depends on it) never ran. Auto-merge wasn't blocked
  because `coverage` was not a required check. Fix: `mkdir -p
  target/llvm-cov/html` ahead of the report invocations in
  `.github/workflows/ci.yml`, and `std::fs::create_dir_all` the
  same path inside `xtask coverage` so local runs surface the
  same shape.

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
