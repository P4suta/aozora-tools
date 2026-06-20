# Troubleshooting

Quick fixes for the most common snags. If none of these help, open a
[bug report](https://github.com/P4suta/aozora-tools/issues/new/choose)
and include the output of `aozora-fmt --version` / `aozora-lsp --version`.

## `aozora-fmt --check` exits 1 but I see no diff

That's by design: plain `--check` only prints the *paths* that would
change (to stderr) and exits `1`. Add `--diff` to see what would change,
or `--list` for just the paths on stdout:

```sh
aozora-fmt --check --diff .
```

## `aozora-fmt .` says "refusing to write N files to stdout"

The default (no-flag) mode streams one document to stdout and only
accepts a single input. To act on many files, pick a mode:

```sh
aozora-fmt --write .   # rewrite in place
aozora-fmt --check .   # verify only
aozora-fmt --list  .   # list the unformatted ones
```

## My `.aozora.txt` / `.afm` files in a directory are skipped

Directory recursion only picks up `*.afm`, `*.aozora`, and `*.aozora.txt`
(case-insensitively), and skips `target/`, `.git/`, and other dotted
entries. A file with a different extension is only formatted when you
name it **explicitly**:

```sh
aozora-fmt --write path/to/oddly-named-file
```

## Shell completions don't work after install

The installer scripts place only the binaries. Install the bundled
completions from the release archive's `completions/` directory into the
location your shell expects — see
[Install → Shell completions](install.md#shell-completions-and-man-pages).
Then restart your shell (or re-run `compinit` on zsh).

## The editor isn't using the language server

1. Confirm the binary runs: `aozora-lsp --version` should print a
   version line (and the embedded parser rev).
2. Confirm your editor launches the right binary — in VS Code the
   extension bundles its own; other editors need `aozora-lsp` on `$PATH`
   or an explicit path in the LSP client config.
3. Turn on logging and check the editor's LSP output:
   `RUST_LOG=aozora_lsp=debug`. See
   [Invocation & environment](../lsp/invocation.md).

## `--version` shows `aozora unknown`

The parser rev is read from `Cargo.lock` at build time. A binary built
outside the workspace (no lockfile reachable) falls back to `unknown`;
this is cosmetic and doesn't affect behaviour. Official release binaries
always carry the real rev.
