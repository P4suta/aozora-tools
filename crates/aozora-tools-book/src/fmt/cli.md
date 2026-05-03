# CLI reference

```text
aozora-fmt [OPTIONS] [PATH]
```

## Arguments

- **`PATH`** — file to format. `-` (or omitted) reads from stdin.

## Options

| Flag | Description |
|---|---|
| `--check`     | Verify-only mode. Exit `1` if the file would be reformatted; print the path to stderr. Conflicts with `--write`. |
| `-w`, `--write` | Rewrite the file in place (no-op when the file is already canonical). Conflicts with `--check`. Requires a real path (not stdin). |
| `-h`, `--help`  | Print help. |
| `-V`, `--version` | Print the formatter's semver and the embedded `aozora` parser tag. |

Default mode (no flag): write the canonicalised form to stdout.

## Exit codes

| Code | Meaning |
|---|---|
| `0`  | Success — or `--check` and the file is already formatted. |
| `1`  | `--check` mode and the file would be reformatted. |
| `2`  | Argument misuse, I/O error, or unrecoverable internal failure. |

## Examples

```sh
# View the canonicalised form without touching the source.
aozora-fmt doc.aozora | less

# Pipe through a chain (e.g. `cat` from a generator).
generate-aozora | aozora-fmt > out.aozora

# Pre-commit hook: bail if any staged file is not canonical.
git diff --cached --name-only --diff-filter=ACMR -z \
  | grep -z '\.aozora$' \
  | xargs -0 --no-run-if-empty -n1 aozora-fmt --check

# CI gate over the whole repo.
fd -e aozora -x aozora-fmt --check {}
```

## Behaviour around symbolic links

`--write` rewrites the file `realpath(PATH)` resolves to. A symlink
in the working tree pointing at a file outside it is followed; the
target is rewritten in place. If you want symlink-safe semantics,
read with `cat`, pipe through `aozora-fmt`, and redirect explicitly:

```sh
cat link.aozora | aozora-fmt > link.aozora
```
