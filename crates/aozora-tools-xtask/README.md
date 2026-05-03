# aozora-tools-xtask

Repo automation for the aozora-tools workspace. Internal-only
(`publish = false`); not shipped as a release artefact.

## Subcommands

```sh
# Sample the LSP burst bench under samply for <SECONDS>
cargo run -p aozora-tools-xtask -- samply lsp-burst 30

# Aggregate a samply trace into a leaf-self-time CLI report
cargo run -p aozora-tools-xtask -- samply analyze /tmp/aozora-lsp-burst-<id>.json.gz
```

The `lsp-burst` runner pre-flights the host environment
(`perf_event_paranoid`, CPU governor, MemAvailable, loadavg)
to keep the trace from being polluted by background work, then
spawns samply with `--unstable-presymbolicate` so the resulting
trace symbolicates cleanly.

The `analyze` post-processor produces a plain-text top-N
self-time-per-leaf table. Plain text means two runs `diff`
cleanly — that's the workflow for "did this change move the
needle on `ts_parser_parse`?".

Full pipeline (capture → symbolicate → summarise → diff)
documented in the
[handbook's Profiling chapter](https://p4suta.github.io/aozora-tools/perf/samply.html).

## Repository

Part of the [aozora-tools](https://github.com/P4suta/aozora-tools)
workspace.
