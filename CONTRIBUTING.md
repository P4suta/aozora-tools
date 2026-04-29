# Contributing to aozora-tools

Thanks for wanting to help. aozora-tools is the editor-surface side of
the [`aozora`](https://github.com/P4suta/aozora) parser ecosystem —
the formatter (`aozora-fmt`), the Language Server (`aozora-lsp`), the
tree-sitter grammar (`tree-sitter-aozora`), and the VS Code extension
under `editors/vscode/`.

## Ground rules

1. **Host toolchain, not Docker.** `cargo`, `bun`, `typos` etc. run
   directly. `rust-toolchain.toml` pins Rust 1.95.0 — `rustup` or
   `dtolnay/rust-toolchain` picks it up automatically.
2. **No warning suppressions.** `#[allow(...)]` / `continue-on-error`
   / `#[cfg_attr(..., allow(...))]` are rejected by review. The
   `dead_code = "deny"` workspace lint is intentional — fix the real
   issue instead of hiding it.
3. **Aozora parser pinning.** The workspace pins
   `aozora` and `aozora-encoding` to a tag on the public sibling
   repo. Do **not** point them at `main` or a branch in a PR; tag
   pinning is what gives us reproducible builds.
4. **TDD with C1 100 % branch coverage as the goal.** Failing test
   first, fix after. The proptest sweep + the `金庫番` guardian suite
   (`crates/aozora-lsp/tests/guardian.rs`) cover panic-resistance,
   idempotence, and concurrency invariants you should not regress.

## First-time setup

```sh
# Rust toolchain + cargo (rustup will read rust-toolchain.toml).
rustup show

# Install lefthook git hooks (pre-commit fmt+clippy+typos,
# commit-msg Conventional Commits, pre-push fmt-check+clippy+test+
# bench-build+doc+typos+vscode-check).
lefthook install

# Verify everything builds + tests green.
cargo test --workspace --all-targets

# VS Code extension dev:
cd editors/vscode
bun install --frozen-lockfile
bun run compile
# Then F5 in VS Code with `editors/vscode/` open → Extension Development Host.
```

## Development loop

```sh
cargo build --workspace                                # debug build
cargo test --workspace --all-targets                   # full test sweep
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo doc --workspace --no-deps --document-private-items
cargo bench --workspace --no-run                       # bench compile check

# VS Code extension:
cd editors/vscode
bun run check    # biome (lint + format check) + tsc --noEmit
bun run compile  # esbuild → out/extension.js
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

- Bump `version` in `Cargo.toml [workspace.package]` + the
  VS Code extension's `editors/vscode/package.json`.
- Update `CHANGELOG.md` (Keep a Changelog format).
- Tag `vX.Y.Z` on `main` after CI is green; the GitHub Pages site
  redeploys on every push to `main`.

## Code of conduct

This project follows the
[Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/);
see `CODE_OF_CONDUCT.md`. Be kind, be specific, and assume the other
person is acting in good faith.
