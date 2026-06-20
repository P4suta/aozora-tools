# Formatter quickstart

`aozora-fmt` follows the `rustfmt` / `prettier` / `gofmt` CLI shape:

```sh
# Print the canonicalised form to stdout (default, single input).
aozora-fmt path/to/doc.aozora

# Verify; non-zero exit if anything would change.
aozora-fmt --check path/to/doc.aozora

# Rewrite in place.
aozora-fmt --write path/to/doc.aozora    # or -w
```

It also takes **many paths and directories** at once (directories are
searched recursively for `.afm` / `.aozora` / `.aozora.txt`):

```sh
aozora-fmt --write .                 # format the whole tree
aozora-fmt --check --diff src/       # show what would change, in colour
aozora-fmt --list .                  # just the unformatted paths (gofmt -l)
aozora-fmt --check --json . > r.json # machine-readable status
```

See the [CLI reference](../fmt/cli.md) for every flag.

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
# Bail the build if anything in the tree is not canonical.
aozora-fmt --check .
```

`--check` mirrors `rustfmt --check`: every rewrite-needed file prints
its path to stderr and the run exits `1`. Because directory recursion
is built in, a single `aozora-fmt --check .` replaces the old
`find … | xargs` pipeline — though that still works if you prefer it.

## What "canonical" means

The formatter parses the document and re-serialises it through the
same `Document::parse ∘ AozoraTree::serialize` path the LSP server
uses for `textDocument/formatting`. The contract is **idempotence**:
running it twice is byte-identical. Any change is a normalising edit
(e.g. implicit ruby `日本《にほん》` → explicit `｜日本《にほん》`);
no rewrite mutates semantic meaning.

See the [Formatting model](../fmt/overview.md) chapter for the full
canonicalisation rules.
