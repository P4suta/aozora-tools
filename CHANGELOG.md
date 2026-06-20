# Changelog

All notable changes to aozora-tools are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/);
this project follows [Semantic Versioning 2.0.0](https://semver.org/),
with the 0.x major-zero contract: any **non-`-pre`** 0.x release may
include API breaks; aim for `0.MINOR.PATCH` reflecting `breaking.feature.fix`
once 1.0 ships.

## [Unreleased]

### Security

- **VS Code preview hardened against HTML injection.** The preview
  webview ran with `enableScripts: true` and no Content-Security-Policy,
  relying solely on the upstream renderer's escaping. It now runs with
  `enableScripts: false`, a strict CSP (`default-src 'none'`), and
  `localResourceRoots: []`, so injected markup is inert even if the
  renderer ever emitted an unescaped tag
  (`editors/vscode/src/preview.ts`).
- **LSP document-size backstop.** Documents above `MAX_DOCUMENT_BYTES`
  (16 MiB) now skip the `O(n)` semantic parse, diagnostics, HTML
  preview, and per-request tree access — publishing one informational
  diagnostic instead — while editing and tree-sitter syntax features
  keep working. Debounced re-parse tasks are coalesced to one per
  document. Together these bound the CPU/memory an adversarial
  multi-hundred-MiB paste can demand
  (`crates/aozora-lsp/src/{segment_cache,backend,state}.rs`).
- **`aozora-fmt --write` safety net.** In-place writes are now guarded
  by a `catch_unwind` (a parser panic exits 2 cleanly and never writes)
  and a fixed-point check (`format(format(x)) == format(x)`), so a
  non-idempotent or panicking parse can no longer corrupt the file
  (`crates/aozora-fmt/src/process.rs`).

### Added

- **`aozora-fmt` multi-file, directory, and diff support.** The
  formatter now accepts any number of paths and recurses directories
  for `*.afm` / `*.aozora` / `*.aozora.txt` (deterministic, sorted,
  de-duplicated; `target/` and dotted entries skipped). New `--diff`
  (coloured unified diff, `--color auto|always|never`), `-l`/`--list`
  (gofmt-style path listing, combinable with `-w`), and `--json`
  (machine-readable `--check` report) modes. `--check` over many files
  reports every offending file and never aborts early; the documented
  0/1/2 exit contract is preserved (`crates/aozora-fmt/src/{cli,discover,process,report}.rs`).
- **`--version` reports the embedded `aozora` parser.** Both binaries
  print `<semver> (aozora <rev> / <tag>)`, e.g.
  `aozora-fmt 0.4.1 (aozora a53c632 / v0.4.1)`, reading the pinned rev
  from `Cargo.lock` at build time (`crates/aozora-{fmt,lsp}/build.rs`).
- **`aozora-lsp` argv handling.** The daemon now answers `--version`
  and `--help` (which also documents `RUST_LOG` and
  `AOZORA_LSP_SLOW_PARSE_US`) and accepts the conventional `--stdio`
  flag, all before the JSON-RPC stream opens (`crates/aozora-lsp/src/cli.rs`).
- **Shell completions + man pages in every release archive.**
  `xtask gen-assets` renders bash/zsh/fish/PowerShell/Nushell
  completions and man pages from the clap CLIs into a committed
  `assets/` tree, which cargo-dist bundles (`dist-workspace.toml`).
  `just gen-assets-check` (wired into `just ci`, CI, and the pre-push
  hook) fails if they drift from the CLIs
  (`crates/aozora-tools-xtask/src/gen_assets.rs`).
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
  `rust-version = "1.96.0"`. Catches MSRV regressions that the
  canonical-toolchain `rust` job would miss.
- **Fuzzing** (`just fuzz-*`, `.github/workflows/fuzz.yml`): cargo-fuzz
  harnesses `aozora-fmt/format_idempotent` and
  `aozora-lsp/edit_pipeline` as nightly-only, out-of-workspace
  sub-crates; a stable `format_source` proptest; and a
  `tests/fuzz_regressions.rs` replay that pins promoted crash inputs on
  every `cargo test` (no nightly). Documented at handbook
  `contrib/fuzzing`.
- **`Justfile`** with host-run developer recipes (`test`, `fmt`,
  `clippy`, `doc`, `deny`, `cov`, `ci`) plus the `fuzz-*` triage family,
  mirroring the sibling aozora / afm repos.
- `#![warn(missing_docs)]` on the `aozora-fmt` public library.
- **Release supply-chain integrity**: the `release` and `release-vscode`
  workflows now attach a CycloneDX SBOM and emit SLSA build-provenance
  attestations for every archive and `.vsix` (verify with
  `gh attestation verify <file> --repo P4suta/aozora-tools`). CodeQL
  default setup already scans `c-cpp` (the tree-sitter `parser.c`),
  JS/TS, and Rust, so no separate code-scanning workflow is added.

### Changed

- **MSRV / pinned toolchain bumped `1.95.0` → `1.96.0`** across
  `rust-toolchain.toml`, the `rust-version`, `clippy.toml` `msrv`, the mise
  manifest, and the CI MSRV gate.
- **`aozora-lsp` public API reduced to `Cli` + `run()`.** The crate used to
  re-export ~50 internal types and functions at its root purely so its own
  tests and benches could reach them. They now live behind a
  `#[doc(hidden)]` `internals` module (no semver guarantee), gated for the
  test/bench/example targets by a new `internals` Cargo feature. Internal
  names were tidied too: `Snapshot`→`DocSnapshot`,
  `MutParagraph`→`ParagraphBuffer`, `BufferState`→`DocBuffer`,
  `DocState`→`OpenDocument`, `LocalTextEdit`→`ByteEdit`,
  `IncrementalDoc`→`TreeSitterDoc`, `Backend`→`AozoraLanguageServer`,
  `SegmentCache`→`ParseCache`, and the three `compute_diagnostics*`
  functions collapsed to `diagnostics_for_source` + `diagnostics_from_aozora`.
- **Workspace version unified with the `aozora` parser (`0.1.3` → `0.4.1`).**
  The tools printed a confusing `0.1.3 (aozora … / v0.4.1)` mismatch; every
  crate now mirrors the pinned parser version and is bumped in lockstep with
  the `aozora` rev. `tree-sitter-aozora` inherits the workspace version (the
  npm grammar package matches); the VS Code extension keeps its independent
  Marketplace version.
- **`aozora-lsp` dropped its runtime dependency on `aozora-fmt`** — the
  formatting handler calls `aozora` directly (byte-identical output),
  removing `aozora-fmt` + `similar`/`walkdir`/`anyhow` from the server's
  dependency tree. `tree-sitter-aozora` is now publishable, so the whole
  workspace can ship to crates.io.
- **`aozora` / `aozora-encoding` dependency bumped `v0.3.0` → `v0.4.0`**,
  picking up the upstream pre-release security hardening (FFI/WASM
  oversized-input rejection, PUA-sentinel neutralisation, parser
  cargo-fuzz harnesses) and the `serialize` I3-idempotency fixes
  (decorative-rule adjacency, leading BOM) that `aozora-fmt --write`
  relies on.

### Fixed

- The `coverage` CI job silently failed since #12: `cargo llvm-cov
  report --lcov` does not create its parent directory, so
  `target/llvm-cov/lcov.info` was never written and the gate never ran.
  Fixed by creating the directory ahead of the report in `ci.yml` and
  in `xtask coverage`.

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
