# aozora-fmt

Idempotent CLI formatter for [aozora-flavored markdown][aozora]
(`.afm` / `.aozora` / `.aozora.txt`).

The formatter is a thin wrapper around the upstream parser:
`Document::parse ∘ AozoraTree::serialize`. Two passes are
guaranteed to produce a byte-identical output (the round-trip
fixed-point invariant is gated by `aozora`'s I3 corpus sweep).

## CLI

```sh
# Read from a file, write the canonical form to stdout
aozora-fmt path/to/doc.afm

# Read from stdin
cat doc.afm | aozora-fmt -

# Verify the file is already canonical (exit 1 otherwise — `rustfmt --check` style)
aozora-fmt --check path/to/doc.afm

# Rewrite in place (no-op when already canonical)
aozora-fmt --write path/to/doc.afm
```

Exit codes: `0` success or check-clean, `1` `--check` would
reformat, `2` any other error.

## Library

The single public entry point is `aozora_fmt::format_source`:

```rust
let canonical = aozora_fmt::format_source(input);
```

`aozora-lsp`'s `textDocument/formatting` handler calls into the
same function so editors and CI gates land on identical output.

## Install

```sh
cargo install --git https://github.com/P4suta/aozora-tools --tag v0.1.3 --locked aozora-fmt
```

Or download a pre-built binary from
[the releases page](https://github.com/P4suta/aozora-tools/releases) —
`aozora-fmt` is bundled in every `aozora-tools-vX.Y.Z-<target>`
archive (Linux x86_64 / macOS arm64 / Windows x86_64).

## Repository

Part of the [aozora-tools][repo] workspace. See the
[workspace README][repo] for the full picture and
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) for the dev loop.

[aozora]: https://github.com/P4suta/aozora
[repo]: https://github.com/P4suta/aozora-tools
