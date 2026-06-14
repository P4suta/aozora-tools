# Fuzzing

aozora-tools fuzzes its two product surfaces with
[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer), in the
same triage style as the sibling `aozora` / `afm` repos.

## Targets

| Crate | Target | Property asserted |
|-------|--------|-------------------|
| `aozora-fmt` | `format_idempotent` | `format_source` never panics and is a fixed point — `format(format(x)) == format(x)` |
| `aozora-lsp` | `edit_pipeline` | the byte/UTF-16 edit splice (`apply_edits`) and the position round-trip never panic on arbitrary (out-of-bounds, inverted, mid-codepoint) input |

The harnesses live in nightly-only sub-crates under `crates/<crate>/fuzz/`,
each with its own `[workspace]` so a plain `cargo build --workspace` never
pulls in `libfuzzer-sys` and the shipped crates stay stable-toolchain and
`forbid(unsafe_code)`.

## Workflow

```sh
just fuzz-quick    aozora-fmt format_idempotent   # 60 s smoke
just fuzz-deep     aozora-fmt format_idempotent   # 5 min pre-flight
just fuzz-marathon aozora-fmt format_idempotent   # 15 min soak
just fuzz-all-quick                               # every target, 60 s each
```

These need a nightly toolchain and `cargo-fuzz`
(`cargo install cargo-fuzz`). The `fuzz` GitHub Actions workflow runs each
target nightly and uploads any crash artifact.

## Triage and regression pinning

On a crash, libFuzzer writes a minimised input to
`crates/<crate>/fuzz/artifacts/<target>/`. Reproduce them all and see the
panic with:

```sh
just fuzz-triage aozora-fmt format_idempotent
```

Once you have a fix, **pin the crash** so it can never silently regress:

```sh
just fuzz-promote aozora-fmt format_idempotent \
  crates/aozora-fmt/fuzz/artifacts/format_idempotent/crash-<hash>
```

`fuzz-promote` copies the input into
`crates/<crate>/tests/fuzz_regressions/<target>/`, where the stable
`tests/fuzz_regressions.rs` integration test replays it on every
`cargo test` / `just test` — **no nightly required** for the regression
case. `just fuzz-status` shows the pending-crash vs pinned-regression count
per target.
