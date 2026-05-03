# Formatter quickstart

`aozora-fmt` follows the `rustfmt` / `prettier` CLI shape. Three modes:

```sh
# Print the canonicalised form to stdout (default).
aozora-fmt path/to/doc.aozora

# Verify; non-zero exit if the file would change.
aozora-fmt --check path/to/doc.aozora

# Rewrite in place.
aozora-fmt --write path/to/doc.aozora    # or -w
```

## Pipe-friendly

`-` (or no path) reads from stdin:

```sh
echo '日本《にほん》' | aozora-fmt
# → ｜日本《にほん》
```

## Exit codes

| Code | Meaning |
|---|---|
| `0`  | Success — or `--check` and the file is already formatted. |
| `1`  | `--check` mode and the file would be reformatted. |
| `2`  | I/O error or argument misuse. |

## CI usage

```sh
# Bail the build if any file is not canonical.
find . -name '*.aozora' -print0 | xargs -0 -n1 aozora-fmt --check
```

The mode mirrors `rustfmt --check` exactly: any rewrite-needed file
prints its path to stderr and exits `1`. Aggregate with `find` /
`fd` and `xargs` for multi-file runs.

## What "canonical" means

The formatter parses the document and re-serialises it through the
same `Document::parse ∘ AozoraTree::serialize` path the LSP server
uses for `textDocument/formatting`. The contract is **idempotence**:
running the formatter twice produces a byte-identical result. A run
that adds or changes a single character is by definition a normalising
edit (e.g. an implicit ruby base `日本《にほん》` becomes the
explicit form `｜日本《にほん》`); no rewrites mutate semantic
meaning.

See the [Formatting model](../fmt/overview.md) chapter for the full
canonicalisation rules.
