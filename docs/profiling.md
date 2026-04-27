# Profiling pipeline

Data-driven optimisation — capture, symbolicate, summarise, diff.

The point of this document is reproducibility. Every measurement
that informs an optimisation should be re-runnable in a single
command, and the post-processing should produce a textual report
that diffs cleanly against past runs. Reading flame graphs by eye
is necessary for shape, but for "did my change move the needle"
we want **numbers next to function names**, captured the same way
each time.

## One-liner workflow

```bash
# Capture
cargo run -p aozora-tools-xtask -- samply lsp-burst 30

# Inspect (CLI top-N — diff-friendly text)
cargo run -p aozora-tools-xtask -- samply analyze /tmp/aozora-lsp-burst-<id>.json.gz

# Inspect (full call hierarchy — Firefox Profiler GUI)
samply load /tmp/aozora-lsp-burst-<id>.json.gz
```

## Pre-flight: trust the trace

`xtask samply lsp-burst` runs four environment checks before
spawning samply. They catch the four most common sources of
**measurement noise that silently invalidates the trace**:

| Check                       | Hard failure (abort)               | Warn-only                          |
|-----------------------------|------------------------------------|------------------------------------|
| `perf_event_paranoid` ≤ 1   | else samply records zero samples   | —                                  |
| CPU governor                | —                                  | warn if not `performance`          |
| MemAvailable ≥ 1 GiB        | —                                  | warn if low (page-fault frames)    |
| loadavg-1m / cpu-count      | —                                  | warn if > 50 % (background work)   |

Hard fixes are printed inline (`echo 1 | sudo tee /proc/sys/...`).
Warnings don't block, but reading them every time is the point —
"my flame graph put `mmap` at the top, that's surprising" is
exactly the moment you remember "oh, I had Chrome compiling in
the background and the trace is mostly page-fault noise."

## Capture: what the runner does

1. Calls `cargo bench --no-run --bench burst -p aozora-lsp` so the
   binary has debug info preserved (`[profile.bench] strip="none",
   debug=1`). Symbolication needs this.
2. Locates the freshly-built bench binary under
   `target/release/deps/burst-<hash>` (newest by mtime).
3. Spawns:
   ```text
   samply record --save-only --no-open
                 --unstable-presymbolicate
                 -o /tmp/aozora-lsp-burst-<runid>.json.gz
                 -r 4000
                 -- <bin> --bench --profile-time <SECONDS>
   ```
   - `--unstable-presymbolicate` writes a `.syms.json` sidecar
     containing the symbol table for every binary touched. Without
     this, function names in the trace stay as raw hex addresses
     (`0x1f547`) and our CLI analyzer can't resolve them.
   - `--profile-time <SECONDS>` is criterion's "spin each bench in
     a tight loop" flag — keeps the trace dominated by the
     measurement code, not by criterion's setup/teardown.
   - `4 kHz` sampling → ~120 k samples per 30 s capture, plenty
     for stable percentages.

## Post-processing: `samply analyze`

The Firefox Profiler GUI is the right tool for **shape** —
"what calls what." For **delta detection** ("did this commit move
3 ms of self-time off this function?") we want a plain-text top-N
that two runs can `diff` against. That's `xtask samply analyze`.

Pipeline:

1. **Decompress** the `.json.gz` trace.
2. **Resolve symbols**: load the `<trace>.syms.json` sidecar,
   build a `(debug_name, rva) → symbol_name` table per binary,
   walk every `funcTable.name` entry whose value is still a raw
   `0x…` address, and replace it with the resolved name.
3. **Aggregate** leaf-frame self-time per function name.
4. **Sort + print** the top 25 per thread, with absolute count and
   percentage of that thread's total samples.

Output shape (excerpt):

```text
# samply trace summary: /tmp/aozora-lsp-burst-1234-5678.json.gz
#
# meta: thread_count=33, total_samples=505876, sampling_interval_ms=0.25
# wall_duration_ms=158.4
# symbols: resolved via /tmp/aozora-lsp-burst-1234-5678.json.syms.json

## thread `burst-...` — top 25 leaves by self-time
   45676    9.6%  ts_parser_parse
   41294    8.7%  ts_subtree_summarize_children
   34175    7.2%  ts_subtree_compress
   26614    5.6%  ts_subtree_release
   21526    4.5%  stack__iter
   17244    3.6%  aozora_lsp::state::DocState::rebuild_snapshot_now
   13996    3.0%  aozora_lsp::line_index::LineIndex::new
   ...
```

## Diff workflow

```bash
# Baseline
cargo run -p aozora-tools-xtask -- samply lsp-burst 30
mv /tmp/aozora-lsp-burst-*.json.gz /tmp/baseline.json.gz
mv /tmp/aozora-lsp-burst-*.json.syms.json /tmp/baseline.json.syms.json
cargo run -p aozora-tools-xtask -- samply analyze /tmp/baseline.json.gz \
  > /tmp/baseline.txt

# Apply your change, rebuild, re-capture
cargo run -p aozora-tools-xtask -- samply lsp-burst 30
cargo run -p aozora-tools-xtask -- samply analyze /tmp/aozora-lsp-burst-*.json.gz \
  > /tmp/variant.txt

# Compare
diff -u /tmp/baseline.txt /tmp/variant.txt | less
# OR side-by-side:
diff -y --width=200 /tmp/baseline.txt /tmp/variant.txt | less
```

Numbers shifting >5 % between runs are real; smaller shifts may be
noise — re-run a couple of times and look for the consistent
direction.

## When the trace looks wrong

| Symptom                                               | Likely cause                                      | Fix                                                            |
|-------------------------------------------------------|---------------------------------------------------|----------------------------------------------------------------|
| Trace is < 100 KB                                     | samply spawned but recorded nothing               | Check `perf_event_paranoid` (preflight should have warned)     |
| All function names are `0x…` hex                      | Sidecar `.syms.json` missing or unreadable         | Re-capture; check `--unstable-presymbolicate` made it through  |
| `_default_morecore` / `mmap` dominate                 | Allocator pressure — your code is allocating hot  | Use `RUSTFLAGS=-Cforce-frame-pointers=yes` for cleaner stacks  |
| Same function listed under multiple `0x…` rows        | Inlining — different inline sites, same function  | Use `cargo --release --profile=bench` (we already do)          |
| `ts_*` functions dominate (~30 %)                     | tree-sitter parse — usually expected on edits     | Validate against bench `subcomponents/ts_parse_full_*`         |

## Adding new profile targets

The runner only knows about `lsp-burst` today. To add a new
target (e.g. `gaiji-extract` to profile just the extract walk),
extend `crates/aozora-tools-xtask/src/main.rs::SamplyTarget`:

```rust
#[derive(Subcommand)]
enum SamplyTarget {
    LspBurst { ... },
    GaijiExtract { iterations: usize },  // new
    Analyze { trace: PathBuf },
}
```

…and add a `samply_gaiji_extract(iterations)` function that
mirrors `samply_lsp_burst`'s shape: preflight, build the bench
binary, spawn samply with `--unstable-presymbolicate`, call
`print_post_run_help`. The analyzer is target-agnostic — it works
on any `.json.gz` from `samply record` regardless of what was
profiled.

## Why this lives in `aozora-tools-xtask`

- The runner needs to invoke `cargo bench --no-run` and locate
  bench binaries — easier from inside the workspace's xtask than
  from a shell wrapper.
- Pre-flight checks read `/proc/...` files; native Rust handles
  the parsing + reporting cleanly without bash brittleness.
- The analyzer's symbol-resolution code is non-trivial (~250 LoC)
  and benefits from `cargo test`-able units. Six tests pin the
  parsing edge cases (hex-address detection, sidecar-path
  derivation, lookup boundary cases).
- Single workspace dependency hop (`cargo run -p
  aozora-tools-xtask`) is easier than maintaining a separate tools
  repo just for profiling.
