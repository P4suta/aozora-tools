# Linting (terminal diagnostics)

Until now the diagnostic engine was reachable only through the editor (the LSP).
`aozora lint` brings it to the terminal — for CI, scripts, and non-VS-Code
editors.

```console
$ aozora lint samples/diagnostics.afm
error: 対応する `［` のない `］` です。
  --> samples/diagnostics.afm:2:16
   |
 2 | 対応する開き括弧のない閉じ括弧］がこの行にあります。
   |                ^^
   |
   = note: aozora::unmatched-close
   = help: run `aozora explain aozora::unmatched-close`
```

The caret underline is **display-width aware**, so it lands correctly under
full-width (CJK) characters. The renderer is
[`annotate-snippets`](https://crates.io/crates/annotate-snippets) — the same one
rustc uses.

## Inputs

`aozora lint [PATHS]...` accepts files, directories (recursed for `*.afm`,
`*.aozora`, `*.aozora.txt`), or `-` / no argument for stdin — the same discovery
rules as the formatter.

## Output formats

| Flag | Output |
|---|---|
| *(default)* | One rendered, caret-underlined snippet per diagnostic. |
| `--quiet` (`-q`) | One line per diagnostic: `path:line:col: severity[code]: summary`. Grep- and CI-friendly. |
| `--json` | A machine-readable report (see below). |

`--color auto\|always\|never` controls ANSI colour; `NO_COLOR` is honoured.

## Exit codes & severities

`0` clean · `1` diagnostics present · `2` error. Warnings alone exit `0` unless
`--error-on-warning` (`-W`) is given; any error-severity diagnostic exits `1`.

## `--stats`

Adds a one-line summary to stderr (or a `stats` object under `--json`):

```text
aozora: scanned 6 files in 4ms — 5 clean, 1 with diagnostics, 0 errored (1 error, 0 warnings)
```

## `--watch`

Re-runs lint on every file change, like a dev server: it clears the screen (when
attached to a TTY), prints a timestamped banner, and re-lints. `Ctrl-C` exits
cleanly. `--watch` needs at least one path and is incompatible with `--json`.

## JSON shape

```json
{
  "version": 1,
  "ok": false,
  "files": [
    {
      "path": "samples/diagnostics.afm",
      "status": "diagnostics",
      "diagnostics": [
        {
          "code": "aozora::unmatched-close",
          "severity": "error",
          "message": "対応する `［` のない `］` です。…",
          "span": { "byte_start": 48, "byte_end": 51 },
          "start": { "line": 2, "column": 16 },
          "end": { "line": 2, "column": 17 }
        }
      ]
    }
  ]
}
```

`status` is `ok`, `diagnostics`, or `error`. Line/column are 1-based character
positions; `span` carries the raw byte range.

## See also

- The [diagnostics catalogue](../lsp/diagnostics.md) documents what each code
  means.
- [`aozora explain <code>`](explain.md) prints the long-form explanation a
  `= help` line points you to.
