# Contributing to aozora-tools

Thanks for wanting to help. aozora-tools is the editor-surface side of
the [`aozora`](https://github.com/P4suta/aozora) parser ecosystem.

## Ground rules

1. **Host toolchain, not Docker.** `cargo`, `bun`, `typos` etc. run
   directly. `rust-toolchain.toml` pins Rust 1.95.0 — `rustup` or
   `dtolnay/rust-toolchain` picks it up automatically.
2. **Justify every suppression.** A `#[allow(...)]` must carry a
   `reason = "..."` (enforced by `clippy::allow_attributes_without_reason`)
   and is reserved for upstream / protocol constraints. `dead_code = "deny"`
   is intentional — fix the real issue instead of hiding it.
3. **Aozora parser pinning.** The workspace pins `aozora` and
   `aozora-encoding` to an immutable commit rev on the public sibling
   repo. Do **not** point them at `main` or a branch in a PR; rev
   pinning is what gives us reproducible builds.
4. **TDD with C1 100 % branch coverage as the goal.** Failing test
   first, fix after. The proptest sweep + the `金庫番` guardian suite
   (`crates/aozora-lsp/tests/guardian.rs`) cover panic-resistance,
   idempotence, and concurrency invariants you should not regress.

## Setup and workflow

```sh
rustup show        # reads rust-toolchain.toml (Rust 1.95.0)
lefthook install   # installs the git hooks (pre-commit, commit-msg, pre-push gate)
cargo test --workspace --all-targets
```

The pre-push hook runs the CI gate: `fmt --check`, clippy `-D warnings`,
tests, `bench --no-run`, doc build, `typos`, and the VS Code `bun run check`.
For the bacon edit loop, profiling, and sanitizers see
[`contrib/dev.md`](./crates/aozora-tools-book/src/contrib/dev.md).

VS Code extension:

```sh
cd editors/vscode
bun install --frozen-lockfile
bun run check     # biome (lint + format) + tsc --noEmit
bun run compile   # esbuild → out/extension.js, then F5 → Extension Development Host
```

## Commit and PR style

- **Conventional Commits** are enforced by `commit-msg` hook:
  `feat:`, `fix:`, `docs:`, `style:`, `refactor:`, `perf:`, `test:`,
  `build:`, `ci:`, `chore:`, `revert:` — scope optional, breaking
  marker `!` optional.
- **Pull requests** should keep one logical change per PR. The bench-diff
  CI job runs `criterion --baseline main` against the LSP burst suite
  on every PR; large numerical regressions show up as a PR comment
  before merge.
- **CODEOWNERS** routes review to `@P4suta`.

## Releasing

Workspace crates and the VS Code extension cut releases independently.

- **Workspace** (`aozora-fmt` / `aozora-lsp`): bump `version` in
  `Cargo.toml [workspace.package]`, update `CHANGELOG.md`, tag `vX.Y.Z`
  on `main`. `release.yml` builds binaries for Linux x86_64 / macOS
  arm64 / Windows x86_64 and attaches them to the GitHub Release.
- **VS Code extension**: bump `version` in `editors/vscode/package.json`
  and tag `vscode-vX.Y.Z` on `main`. `release-vscode.yml` packages a
  platform-specific `.vsix` per supported target and publishes to the
  Marketplace (Open VSX is opportunistic).
- The GitHub Pages rustdoc site redeploys on every push to `main` —
  no separate tag step.

## Code of conduct

This project follows the
[Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/);
see `CODE_OF_CONDUCT.md`. Be kind, be specific, and assume the other
person is acting in good faith.
