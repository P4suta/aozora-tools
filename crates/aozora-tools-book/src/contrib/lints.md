# Lint posture

Lint configuration lives in `Cargo.toml`'s `[workspace.lints]` and
`clippy.toml` at the workspace root. Both apply to every member
crate by default; per-crate carve-outs are explicit and rare.

## Three principles

1. **Fix the code, don't silence the lint.** `#[allow(...)]` is the
   last resort. The first try is always to change the code so the
   lint no longer fires; the second is to argue (in a workspace-wide
   `[workspace.lints]` change) that the lint is wrong for this
   repo's idioms. A scattered `#[allow]` decays the gate's value.
2. **Lints catch bug classes, not stylistic taste.** Each enabled
   restriction lint targets a class of bugs (e.g.
   `let_underscore_must_use` catches silent `Result` drops). The
   `[workspace.lints.clippy]` block in `Cargo.toml` lists the
   bug-class each restriction lint addresses inline.
3. **The CI gate and the local gate run the same command.** No
   "looser local rules" — `cargo clippy --all-targets --all-features
   -- -D warnings` reproduces the CI invocation exactly.

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
  name is the domain term (`paragraph::ParagraphSnapshot`). The
  refactor cost is not worth the lint's value.
- **`missing_const_for_fn`** — allowed. Forces `const fn`
  discipline on hot paths where the const-eligibility is incidental.
- **`redundant_pub_crate`** — allowed because it directly
  contradicts `unreachable_pub` from rustc, and the latter (narrow
  visibility) is the more useful signal.
- **`multiple_crate_versions`** — allowed in the cargo group
  because transitive dep version mismatches (e.g. `unicode-width
  0.1` vs `0.2` pulled in by different deps) are not our problem to
  fix locally.

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

Only one crate has an `[lints.rust]` override:

- **`tree-sitter-aozora`** — `unused = "allow"` for the generated
  `parser.c` warnings. The override is scoped to this crate so
  hand-written code in `bindings/rust/` keeps the workspace
  defaults.

That is the entire exception list. Other members inherit the
workspace lints exactly.

## Adding a new lint

1. Add the entry to `[workspace.lints.*]` in the workspace
   `Cargo.toml` with an inline comment naming the bug class.
2. Run `cargo clippy --workspace --all-targets --all-features
   -- -D warnings` and fix the call sites the new lint flags.
3. If the call-site fix would distort the code, argue for an
   exemption in the same PR: either drop the new lint or add a
   carve-out in the workspace `[workspace.lints]` block (not a
   scattered `#[allow]`).
