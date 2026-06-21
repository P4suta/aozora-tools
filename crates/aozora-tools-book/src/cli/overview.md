# The `aozora` CLI

`aozora` is the unified command-line entry point for the toolset. One binary,
several subcommands:

| Command | What it does |
|---|---|
| `aozora fmt` | Idempotent formatter — canonicalise documents. Same engine as the standalone [`aozora-fmt`](../fmt/cli.md). |
| `aozora lint` (alias `check`) | [Report diagnostics in the terminal](lint.md) with rustc-style carets, JSON, or a terse one-liner. |
| `aozora render` | [Render a document to HTML](render.md). |
| `aozora explain <code>` | [Explain a diagnostic code](explain.md) in long form. |
| `aozora lsp` | Run the language server over stdio (for editors). Same daemon as the standalone `aozora-lsp`. |
| `aozora completions <shell>` | Print a shell completion script (bash, zsh, fish, PowerShell, Nushell). |
| `aozora man [<subcommand>]` | Print a man page (troff) for the tool or a subcommand. |

Run `aozora --help`, or `aozora <command> --help`, for the full flag list. Every
help screen ends with a copy-pasteable **Examples** block.

## Exit codes

`fmt` and `lint` share a three-valued contract:

| Code | Meaning |
|---|---|
| `0` | Clean — nothing to format / no diagnostics. |
| `1` | Findings — `fmt` would reformat, or `lint` found a diagnostic. |
| `2` | Error — an input could not be read, or the parser failed. |

For `lint`, warning-only documents exit `0` by default; pass `--error-on-warning`
(`-W`) to make warnings fail the run too. See [Linting](lint.md).

## Relationship to the standalone binaries

The `aozora-fmt` and `aozora-lsp` binaries are still built and published. They
are unchanged, so editors and scripts that invoke `aozora-lsp` by name keep
working. `aozora fmt` / `aozora lsp` are the same code reached through the
umbrella — there is one implementation per surface, not a fork.

## Migration

| Before | Now (either works) |
|---|---|
| `aozora-fmt <args>` | `aozora fmt <args>` |
| `aozora-lsp` | `aozora lsp` |

New capabilities — `lint`, `render`, `explain` — are only available through the
`aozora` umbrella.
