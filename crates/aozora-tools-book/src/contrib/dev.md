# Development loop

The fast path through a contributor day.

## Once per machine

```sh
git clone https://github.com/P4suta/aozora-tools
cd aozora-tools

# Rust toolchain (matches rust-toolchain.toml)
rustup show           # auto-installs the pinned channel + components

# Optional but strongly recommended
cargo install --locked cargo-nextest cargo-deny git-cliff bacon mdbook mdbook-mermaid
# Or via mise / cargo-binstall for faster installs.

# VS Code extension dependencies
(cd editors/vscode && bun install --frozen-lockfile)

# Lefthook git hooks
lefthook install
```

## Edit-build-test loop

The recommended local loop is `bacon`, which watches the workspace
and re-runs the chosen job on save:

```sh
bacon            # default: cargo check
bacon clippy     # cargo clippy --all-targets --all-features -- -D warnings
bacon test       # cargo test --workspace
bacon doc        # cargo doc --workspace --no-deps --document-private-items
```

`bacon.toml` defines these jobs; switch between them inside the
TUI with `c` / `t` / `d`.

## Pre-commit gate

`lefthook` runs *gentle* checks on commit: format-and-restage
(`cargo fmt --all` writes; `bun run check:fix` writes), `cargo
clippy`, `typos`. The pre-push hook is **strict**: `cargo fmt
--check`, full `clippy --all-features`, the workspace test suite,
`cargo bench --no-run`, `cargo doc`, `typos`, and the VS Code
extension `bun run check`.

`jj` colocated repos bypass git hooks. The pre-push hook is the
hard gate — it runs whether you commit through `git` or `jj`.

## Local CI parity

Reproduce the CI gate before pushing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-targets --locked
cargo test --doc --workspace --locked
cargo doc --workspace --no-deps --document-private-items --locked
cargo check --benches --workspace --locked
cargo deny --all-features --manifest-path Cargo.toml check
typos
(cd editors/vscode && bun run check)
```

The handbook itself has an additional gate:

```sh
mdbook-mermaid install crates/aozora-tools-book
mdbook build crates/aozora-tools-book
lychee --config crates/aozora-tools-book/lychee.toml \
       crates/aozora-tools-book/book/
```

## Where things live

```
.
├── Cargo.toml             ← workspace + [workspace.lints]
├── deny.toml              ← cargo-deny policy
├── bacon.toml             ← bacon jobs
├── cliff.toml             ← git-cliff CHANGELOG / Release notes
├── rustfmt.toml           ← formatter rules (stable rustfmt only)
├── clippy.toml            ← clippy thresholds + restriction lists
├── lefthook.yml           ← git hooks (pre-commit, commit-msg, pre-push)
├── rust-toolchain.toml    ← pinned toolchain channel
├── _typos.toml            ← typos allow-list
├── crates/
│   ├── aozora-fmt/        ← idempotent formatter (lib + CLI)
│   ├── aozora-lsp/        ← LSP server
│   ├── tree-sitter-aozora/← tree-sitter grammar
│   ├── aozora-tools-xtask/← repo automation (samply, preflight)
│   └── aozora-tools-book/ ← this handbook (excluded from workspace)
├── editors/
│   └── vscode/            ← VS Code extension (TypeScript + esbuild)
├── samples/               ← hand-written .afm test inputs
├── scripts/
│   ├── pgo-build.sh       ← PGO + optional BOLT release builds
│   └── sanitizers.sh      ← miri / tsan / asan harness
└── docs/
    └── adr/               ← architecture-decision history (background)
```

## Issue and PR templates

Bugs, features, and configs each have their own
`.github/ISSUE_TEMPLATE/*.yml`. PRs auto-fill from
`.github/PULL_REQUEST_TEMPLATE.md` — keep the test-plan section in
the description even for one-line changes; "I ran the gates locally"
counts.
