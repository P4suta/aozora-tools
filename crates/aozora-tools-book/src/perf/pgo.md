# Release profile & PGO

The workspace ships three release-class profiles. Choose by what
you are building.

## `[profile.release]`

The default optimisation profile. Used by `cargo build --release`,
`cargo install --locked`, and the `release.yml` workflow that
publishes the GitHub Releases binaries.

```toml
lto           = "thin"
codegen-units = 1
strip         = "symbols"
opt-level     = 3
```

`thin` LTO with one codegen unit captures most of the `lto = "fat"`
win for much less link time, which matters because this profile is
also what `cargo install` end-users run.

## `[profile.dist]`

Distribution profile for binaries shipped *inside* the VS Code
extension `.vsix`. Optimises for **size** over speed:

```toml
inherits      = "release"
opt-level     = "z"
lto           = "fat"
codegen-units = 1
strip         = "symbols"
```

Why size: the `aozora-lsp` bundled inside a `.vsix` is downloaded
once on install and runs as a long-lived subprocess, so on-disk and
over-network size matters more than per-request CPU. `lto = "fat"`
gives the size win that `thin` can't reach.

`panic = "unwind"` is preserved (no `panic = "abort"`) so tokio's
task isolation continues to work — a panic in one request does not
tear down the whole server.

## `[profile.bench]`

Inherits `release` but keeps debug info so `samply` can symbolicate
stack frames:

```toml
inherits = "release"
strip    = "none"
debug    = 1
```

`debug = 1` (`line-tables-only`) is the minimum that gives `samply
analyze` resolvable function names; `debug = 2` would inflate binary
size without buying the profiler anything.

## `[profile.dev]` tuning

```toml
[profile.dev]
split-debuginfo = "unpacked"

[profile.dev.package."*"]
opt-level = 1
```

`split-debuginfo = "unpacked"` keeps debug info in sibling `.dwo`
files instead of inlining it into every `.o`, which shortens link
time on incremental rebuilds.

`[profile.dev.package."*"] opt-level = 1` lightly optimises
*dependency* crates only — workspace members keep `opt-level = 0`
for fast iteration and breakpoint quality. Tests and benches that
spend most of their CPU inside dependencies (proptest, criterion,
tower-lsp) run faster without losing debugger fidelity on
first-party code.

## Profile-Guided Optimisation (PGO)

The `scripts/pgo-build.sh` script drives an end-to-end PGO build
of `aozora-fmt` and `aozora-lsp`. Three phases:

1. **Instrumented build** — `cargo pgo build -- -p aozora-fmt -p aozora-lsp`
   plus the `aozora-lsp::burst` bench.
2. **Profile collection** — runs the instrumented `aozora-fmt` over
   every file in `samples/` three times, then runs the LSP burst
   bench in `--quick` mode.
3. **Optimised rebuild** — `cargo pgo optimize build -- -p aozora-fmt -p aozora-lsp`.

Optional fourth phase (Linux x86_64 only): **llvm-bolt** post-link
layout optimisation. Detected automatically; skipped with a hint
when `llvm-bolt` is not on `$PATH`.

Verify any gain on your own hardware with:

```sh
hyperfine --warmup 3 \
  'cargo run --release -p aozora-fmt -- samples/bouten.afm' \
  './target/x86_64-unknown-linux-gnu/release/aozora-fmt samples/bouten.afm'
```

Requirements: `cargo install cargo-pgo` and `rustup component add
llvm-tools-preview`.

## When to use which profile

| Scenario | Profile | Command |
|---|---|---|
| Local dev loop | `dev` | `cargo build` / `cargo test` |
| Running benches | `bench` | `cargo bench` |
| Profiling a hot path | `bench` | `cargo run -p aozora-tools-xtask -- samply lsp-burst 30` |
| Building Release binaries | `release` | `cargo build --release` |
| Building VS Code extension bundle | `dist` | `cargo build --profile dist --target <triple> -p aozora-lsp` |
| Squeezing the last few percent | `release` + PGO + (optional) BOLT | `scripts/pgo-build.sh` |
