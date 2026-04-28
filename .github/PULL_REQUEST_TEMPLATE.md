## Summary

<!-- One or two sentences: what this PR changes and why. -->

## Type of change

- [ ] Bug fix
- [ ] New feature (LSP capability, formatter rule, VS Code command, tree-sitter highlight, …)
- [ ] Refactor (no behaviour change)
- [ ] Documentation / ADR
- [ ] CI / developer tooling
- [ ] `aozora` parser pin bump

## Affected component

- [ ] `crates/aozora-fmt`
- [ ] `crates/aozora-lsp`
- [ ] `crates/tree-sitter-aozora`
- [ ] `crates/aozora-tools-xtask`
- [ ] `editors/vscode/`
- [ ] CI / repo plumbing

## Checklist

- [ ] `cargo test --workspace --all-targets` passes locally.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all -- --check` passes (lefthook pre-commit reformats automatically).
- [ ] `cargo doc --workspace --no-deps --document-private-items` builds with `RUSTDOCFLAGS=-D warnings`.
- [ ] `bun run check` in `editors/vscode/` passes (if VS Code extension touched).
- [ ] `cargo bench --workspace --no-run` builds (if benches touched).
- [ ] Added or updated tests that exercise the change (proptest, integration, or LSP-protocol level).
- [ ] Updated `CHANGELOG.md` under `[Unreleased]` (or stated why it doesn't need a changelog entry).
- [ ] Commit messages follow Conventional Commits (commit-msg hook enforces).
- [ ] If bumping the `aozora` parser pin: aligned `tag = "vX.Y.Z"` and `version = "X.Y.Z"` in both `aozora` and `aozora-encoding` workspace deps; the matching CHANGELOG entry calls out which parser features unlocked.

## How to test

<!-- Reviewer-facing repro steps. For LSP work, include a sample
buffer + the LSP request that exercises it. For formatter work,
include the input ↔ canonical-output pair. -->
