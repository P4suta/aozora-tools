# Lint posture

Lint configuration lives in `Cargo.toml`'s `[workspace.lints]` and
`clippy.toml` at the workspace root, and applies to every member crate.

## Three principles

1. **Fix the code, don't silence the lint.** `#[allow(...)]` is the
   last resort; prefer changing the code, or arguing the lint wrong
   for this repo's idioms in a `[workspace.lints]` change. Scattered
   `#[allow]` decays the gate's value.
2. **Lints catch bug classes, not stylistic taste.** Each enabled
   restriction lint targets a class of bugs (e.g.
   `let_underscore_must_use` catches silent `Result` drops), named
   inline next to the lint in `[workspace.lints.clippy]`.
3. **The CI gate and the local gate run the same command** —
   `cargo clippy --all-targets --all-features -- -D warnings`.

## What is enabled

- `[workspace.lints.rust]` — strong-signal warnings (missing-debug,
  trivial-casts, lifetime hygiene), plus `unsafe_code = forbid` and
  `non_ascii_idents = deny`.
- `[workspace.lints.rustdoc]` — broken intra-doc links are deny;
  invalid HTML / codeblock attributes / Rust codeblocks are warn.
- `[workspace.lints.clippy]` — `pedantic` + `nursery` + `cargo`
  groups all enabled at warn level. A hand-picked set of
  `restriction` lints (each chosen for the bug class it catches)
  is bumped to warn individually.

The full list is in
[`Cargo.toml`](https://github.com/P4suta/aozora-tools/blob/main/Cargo.toml);
read the inline comments next to each lint for the rationale.

## Carve-outs

- **`module_name_repetitions`** — allowed. Noisy when the module
  name is the domain term (`paragraph::ParagraphSnapshot`).
- **`missing_const_for_fn`** — allowed. Forces `const fn` discipline
  where const-eligibility is incidental.
- **`redundant_pub_crate`** — allowed; it contradicts rustc's
  `unreachable_pub`, and narrow visibility is the more useful signal.
- **`multiple_crate_versions`** — allowed in the cargo group;
  transitive dep version mismatches are not ours to fix locally.

## Clippy thresholds

`clippy.toml` tunes a few thresholds beyond clippy's defaults:

| Threshold | Value | Why |
|---|---|---|
| `too-many-arguments-threshold` | 4 | Encourages struct extraction at smaller scopes. |
| `too-many-lines-threshold`     | 80 | Pushes long functions toward extracted helpers. |
| `cognitive-complexity-threshold` | 18 | Stricter than clippy's 25 default. |
| `disallowed-methods` | `std::mem::forget`, `std::env::set_var`, `std::process::exit` | Each one has a domain-specific footgun: `forget` leaks Drop types; `set_var` is unsound after multi-thread init in Rust 1.95+; `exit` skips Drop entirely. |
| `disallowed-types` | `std::sync::RwLock` | Prefer `parking_lot::RwLock` for the same reasons we prefer `parking_lot::Mutex` (no poisoning, faster contention). |

## Per-crate exceptions

None — every workspace member inherits the workspace lints exactly.
The generated `parser.c` in `tree-sitter-aozora` is C, not Rust; its
clang warnings are silenced by `-Wno-*` flags in that crate's
`build.rs`.

## Unused dependencies

`dead_code = "deny"` flags unreachable *code*, but it cannot see a crate
declared in `Cargo.toml` that nothing imports. `cargo-shear` closes that
gap: it parses every source file for actual `use` sites and reports any
dependency with none. It runs as the CI `shear` job, in the pre-push
hook, through `just shear`, and inside `just ci`.

A genuine false positive — a dependency reached only through a macro, a
`cfg`-gated path, or a feature-gated optional dep — is recorded under
`[workspace.metadata.cargo-shear]` or a crate's
`[package.metadata.cargo-shear] ignored`, with a comment stating why
(see `aozora-lsp`'s `shuttle` entry). Dropping the crate is always
preferred over an ignore entry.

## Adding a new lint

1. Add the entry to `[workspace.lints.*]` in the workspace
   `Cargo.toml` with an inline comment naming the bug class.
2. Run `cargo clippy --workspace --all-targets --all-features
   -- -D warnings` and fix the call sites the new lint flags.
3. If the call-site fix would distort the code, argue for an
   exemption in the same PR: either drop the new lint or add a
   carve-out in the workspace `[workspace.lints]` block (not a
   scattered `#[allow]`).
