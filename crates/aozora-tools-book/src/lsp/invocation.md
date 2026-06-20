# Invocation & environment

`aozora-lsp` is a daemon: it speaks the [Language Server Protocol] over
stdio and is normally launched by your editor, not by hand. stdout is
reserved for the JSON-RPC wire protocol; all logging goes to stderr.

```text
aozora-lsp [OPTIONS]
```

## Options

| Flag | Description |
|---|---|
| `--stdio`         | Speak LSP over stdio. Accepted for editor compatibility; it's the only transport, so the flag is a no-op. |
| `-h`, `--help`    | Print help (including the environment variables below) and exit. |
| `-V`, `--version` | Print the server's semver and the pinned `aozora` parser rev/tag, e.g. `aozora-lsp 0.4.1 (aozora a53c632 / v0.4.1)`, and exit. |

`--help` and `--version` print and exit **before** the server opens the
stdio stream, so they never interfere with the protocol. An unknown flag
is rejected with a usage error (exit `2`).

## Environment variables

| Variable | Default | Effect |
|---|---|---|
| `RUST_LOG`                 | `warn`   | tracing filter, e.g. `aozora_lsp=debug`. Logs go to **stderr**. |
| `AOZORA_LSP_SLOW_PARSE_US` | `100000` | Per-parse latency threshold in microseconds; parses slower than this log a warning (useful when profiling large documents). |

## Checking the binary

Running `aozora-lsp --version` is the quickest way to confirm an editor
is launching the server you expect, and which upstream parser it embeds —
include that line in bug reports. To watch what the server is doing, set
`RUST_LOG` and inspect your editor's LSP log:

```sh
RUST_LOG=aozora_lsp=debug aozora-lsp --version   # smoke test on the CLI
```

[Language Server Protocol]: https://microsoft.github.io/language-server-protocol/
