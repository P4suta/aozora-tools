# CLI reference

```text
aozora-fmt [OPTIONS] [PATH]...
```

## Arguments

- **`PATH...`** — files or directories to format. `-` (or no path at
  all) reads a single document from stdin. Directories are searched
  **recursively** for `*.afm`, `*.aozora`, and `*.aozora.txt` files
  (`target/`, `.git/`, and other dotted entries are skipped). Explicitly
  named files are always processed regardless of extension. Paths are
  de-duplicated and processed in sorted order, so output is deterministic.

You cannot mix `-` with real paths. Globs (`**/*.afm`) are expanded by
your shell; on shells without globbing, pass a directory and let the
recursive walk find the files.

## Options

| Flag | Description |
|---|---|
| `--check`         | Verify-only. Exit `1` if **any** input would change; print each such path to stderr. |
| `-w`, `--write`   | Rewrite changed files in place (no-op when already canonical). Requires real paths, not stdin. |
| `--diff`          | Print a unified diff for every file that would change. Implies `--check`; diffs go to stdout. |
| `-l`, `--list`    | Print only the paths that would change (à la `gofmt -l`), one per line, to stdout. Combine with `-w` to list *and* rewrite. |
| `--json`          | Emit the `--check` result as a machine-readable JSON object. Implies `--check`. |
| `--color <WHEN>`  | Colourise `--diff` output: `auto` (default — colour when stdout is a TTY, honouring `NO_COLOR`), `always`, or `never`. |
| `-h`, `--help`    | Print help. |
| `-V`, `--version` | Print the formatter's semver **and** the pinned `aozora` parser rev/tag, e.g. `aozora-fmt 0.1.3 (aozora a53c632 / v0.4.1)`. |

Default mode (no `--check`/`--write`/`--list`): write the canonicalised
form to stdout. This only accepts a **single** input; pointing it at two
or more files is an error (use `--write`, `--check`, or `--list`).

## Exit codes

| Code | Meaning |
|---|---|
| `0`  | Success — every input was already formatted (or written / listed). |
| `1`  | `--check`: at least one input would be reformatted. |
| `2`  | Argument misuse, I/O error, missing path, or an unrecoverable internal failure. |

Errors never abort the run early: a missing or unreadable file is
reported and processing continues for the rest. When a run both finds
files that would change *and* hits an error, the error wins — exit `2`.

## JSON output

`--check --json` prints one object to stdout (exit code unchanged):

```json
{
  "version": 1,
  "formatted": false,
  "files": [
    { "path": "chapter1.afm", "status": "would_reformat" },
    { "path": "chapter2.afm", "status": "ok" },
    { "path": "broken.afm", "status": "error", "message": "reading broken.afm: …" }
  ]
}
```

`formatted` is `true` only when every input was already canonical.
`status` is one of `ok`, `would_reformat`, or `error`.

## Examples

```sh
# Format one file to stdout.
aozora-fmt doc.aozora | less

# Rewrite a whole project in place.
aozora-fmt --write .

# CI gate over the repo: non-zero if anything is unformatted.
aozora-fmt --check .

# Show what would change, in colour.
aozora-fmt --check --diff src/

# List unformatted files for a script to consume.
aozora-fmt --list . | xargs -r some-tool

# Machine-readable status for tooling.
aozora-fmt --check --json . > report.json
```

## Shell completions and man pages

Pre-built release archives bundle shell completions
(`completions/`) and man pages (`man/aozora-fmt.1`). See
[Install](../getting-started/install.md#shell-completions-and-man-pages)
for where to drop them.

## Behaviour around symbolic links

During directory recursion, symlinks are **not** followed (matching
`WalkDir::follow_links(false)`), so symlink loops can't trap the walk.
A symlink passed **explicitly** is followed by the OS: `--write` rewrites
the file it resolves to. For symlink-safe semantics, read with `cat`,
pipe through `aozora-fmt`, and redirect explicitly:

```sh
cat link.aozora | aozora-fmt > link.aozora
```
