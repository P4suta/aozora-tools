# Sanitizers

`scripts/sanitizers.sh` is the on-demand sanitizer harness. It
wraps a nightly Rust toolchain plus one of `miri` / `tsan` / `asan`
around `cargo test`, with the right `RUSTFLAGS` and `cfg` set up to
make the sanitizer's diagnostics actionable.

## Three modes

| Mode  | Target speed (relative) | What it catches                                   |
|-------|-------------------------|---------------------------------------------------|
| `miri`| 10–100× slower         | UB (alignment, uninit reads, OOB pointers, stacked-borrows violations) |
| `tsan`|  2–10× slower          | Data races between threads                       |
| `asan`|     ~3× slower         | Use-after-free, heap buffer overflows, memory leaks |

## Usage

```sh
scripts/sanitizers.sh miri
scripts/sanitizers.sh tsan
scripts/sanitizers.sh asan
```

Each invocation:

1. Activates the nightly toolchain via `rustup run nightly`.
2. Adds the sanitizer-specific component if missing
   (`miri`, `rust-src` for tsan/asan).
3. Sets the `RUSTFLAGS` / `MIRIFLAGS` the sanitizer needs.
4. Runs `cargo test --workspace --all-targets`.

Results stream to the terminal; the script exits with the first
non-zero test result.

## When to run them

- **`miri`** — after any change to `aozora-lsp`'s state model
  (`OpenDocument`, `DocBuffer`, `DocSnapshot`), to the rope buffer
  integration, or to anything involving `Arc<DashMap>`. miri is the
  tool that catches the unsoundness `cargo test` cannot.
- **`tsan`** — after a change that touches the lock graph
  (`parking_lot::Mutex`, `ArcSwap`, the debounced reparse task).
  Pairs well with the shuttle randomised model checker behind the
  `shuttle-tests` feature flag (run both — they catch overlapping
  but not identical bug classes).
- **`asan`** — rarely needed for pure Rust but high-signal when
  touching `tree-sitter-aozora`'s C parser surface or any FFI
  boundary. Picks up use-after-free in C code that miri cannot
  see.

## What the harness does **not** do

- It does not run on CI by default — sanitizers' wall-time would
  break the CI budget. Run them on demand before merging changes
  that touch the unsound-prone surface above.
- It does not run on Windows or non-x86_64 macOS. miri is
  cross-platform but tsan/asan are tied to LLVM sanitizer support;
  the script bails early on unsupported platforms.

## Reading sanitizer output

- **miri** — UB reports include the pointer's stacked-borrows
  history; the `Inside ` / `Outside ` markers locate the violating
  borrow. Most miri-only failures in `aozora-lsp` are around the
  `bumpalo` arena boundary — the `Document` cannot escape its
  `with_tree` closure for exactly this reason.
- **tsan** — race reports name the two conflicting stack traces.
  The usual pattern: a `parking_lot::Mutex` guard dropped on one
  thread while another reads the data without the same mutex. The
  fix is almost always to extend the mutex hold, not to add atomics.
- **asan** — use-after-free reports include both allocation and
  deallocation stacks. Inside `tree-sitter-aozora`'s C parser these
  are usually edits to a tree whose subtree was reclaimed.

## Pairing with the shuttle model checker

The `aozora-lsp` test suite includes a `shuttle`-driven
randomised-schedule test (`tests/shuttle_doc_state.rs`, gated
behind the `shuttle-tests` feature):

```sh
cargo test --features shuttle-tests --test shuttle_doc_state
```

Shuttle explores arbitrary interleavings against the
`Arc<DashMap<Url, OpenDocument>>` and pins correctness invariants. Run
both: tsan answers "is the runtime safe?", shuttle answers "does
the model hold?".
