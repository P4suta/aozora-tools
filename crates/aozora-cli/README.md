# aozora-cli

The `aozora` command-line tool — one binary for the whole
[aozora-tools](https://github.com/P4suta/aozora-tools) workflow over
aozora-flavored-markdown (青空文庫 notation).

```console
$ aozora fmt -w chapter.afm                 # format in place
$ aozora lint samples/                      # terminal diagnostics (rustc-style)
$ aozora lint --json doc.afm | jq           # machine-readable diagnostics
$ aozora explain aozora::unclosed-bracket   # explain a diagnostic code
$ aozora render doc.afm > doc.html          # render to HTML
$ aozora lsp                                # run the language server (for editors)
```

Subcommands:

| Command | Purpose |
| --- | --- |
| `aozora fmt` | Idempotent formatter (also shipped standalone as `aozora-fmt`). |
| `aozora lint` (`check`) | Report diagnostics in the terminal; `--json`, `--quiet`, `--watch`. |
| `aozora render` | Render a document to HTML (`--standalone`, `-o`, `--open`). |
| `aozora explain <code>` | Long-form explanation of a diagnostic code. |
| `aozora lsp` | Language server over stdio (also standalone as `aozora-lsp`). |
| `aozora completions <shell>` / `aozora man` | Print completion scripts / man pages. |

Exit codes for `fmt`/`lint`: `0` clean, `1` findings, `2` error.

The `aozora-fmt` and `aozora-lsp` binaries remain available for backward
compatibility (e.g. editors that launch `aozora-lsp` by name).

Licensed under Apache-2.0 OR MIT.
