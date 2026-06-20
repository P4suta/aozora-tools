# Development loop

## Onboarding

### Recommended: dev container / Codespaces

Open the repo in a dev container (VS Code *Dev Containers: Reopen in
Container*) or a Codespace. `.devcontainer/` provisions the pinned
toolchain (rustup + mise), installs the lefthook git hooks, installs the
VS Code extension deps, and warms the cargo cache. When it finishes you
have a ready workspace; `just doctor` prints a green/red readiness report.

Rust is pinned by `rust-toolchain.toml`; the container installs it via
rustup (so there is exactly one effective Rust pin) and lets mise
provision every *other* tool from `mise.toml` + `.config/mise/config.toml`
(`MISE_DISABLE_TOOLS=rust`).

### Host toolchain (also supported)

```sh
git clone https://github.com/P4suta/aozora-tools
cd aozora-tools

just bootstrap        # mise install (pinned tools) + lefthook install
just doctor           # verify; prints the next command for any gap
```

`just bootstrap` provisions everything in the mise manifest (Rust per
`rust-toolchain.toml`, plus just, lefthook, typos, committed, actionlint,
bun, cargo-nextest, cargo-deny, cargo-llvm-cov) and installs the git
hooks. Prefer to drive Rust yourself? `rustup show` reads
`rust-toolchain.toml` and materialises the pinned channel + components.
`bacon` and the rest (`git-cliff`, `mdbook`, …) come in via mise /
`cargo-binstall`; cargo-fuzz needs nightly and is installed by the fuzz
workflow, not the default set.

## Edit-build-test loop

`bacon` watches the workspace and re-runs the chosen job on save:

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
`cargo bench --no-run`, `cargo doc`, `typos`, `gen-assets --check`,
and the VS Code extension `bun run check`.

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
cargo run -p aozora-tools-xtask --locked -- gen-assets --check
cargo deny --all-features --manifest-path Cargo.toml check
typos
(cd editors/vscode && bun run check)
```

`just ci` runs the same set in one shot.

## Generated assets (completions + man pages)

The shell completions and man pages under `assets/` are **generated**
from the clap CLIs, committed, and bundled into release archives by
cargo-dist. After any change to a binary's arguments, regenerate and
commit them:

```sh
just gen-assets        # rewrites assets/completions/ + assets/man/
```

`just ci`, the CI `assets up to date` step, and the lefthook pre-push
hook all run `gen-assets --check`, which fails if the committed tree has
drifted from the current CLIs.

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
├── .devcontainer/         ← recommended onboarding (Dockerfile + scripts)
├── .vscode/               ← root-window editor config (rust-analyzer, debug)
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
├── assets/                ← generated completions + man pages (just gen-assets)
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
