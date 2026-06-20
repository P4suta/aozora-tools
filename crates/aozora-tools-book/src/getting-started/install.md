# Install

Three install paths cover every supported workflow.

## VS Code (most users)

1. Open the Extensions panel (`Ctrl+Shift+X`).
2. Search for **aozora** and install
   `yasunobu-sakashita.aozora` (also published on
   [Open VSX](https://open-vsx.org/)).
3. Open any `.aozora`, `.afm`, or `.aozora.txt` file.

The extension bundles a platform-specific `aozora-lsp` binary inside
the `.vsix`, so there is no separate language-server install. Linux
x86_64 GNU, macOS arm64, and Windows x86_64 MSVC are the published
targets; other platforms can install from source (below) and point
the extension at the local binary via the `aozora-lsp.serverPath`
setting.

## Pre-built binaries (any LSP-capable editor)

Each release attaches `aozora-fmt` + `aozora-lsp` archives for the
three primary platforms.

```sh
# Pick your platform from the latest release:
#   https://github.com/P4suta/aozora-tools/releases
#
#   aozora-tools-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
#   aozora-tools-vX.Y.Z-aarch64-apple-darwin.tar.gz
#   aozora-tools-vX.Y.Z-x86_64-pc-windows-msvc.zip
#
# Verify with the matching SHA256SUMS file before extracting.
sha256sum --check SHA256SUMS
```

The archive contains `aozora-fmt`, `aozora-lsp`, `LICENSE-APACHE`,
`LICENSE-MIT`, `README.md`, plus a `completions/` directory (shell
completions) and `man/` (man pages — see below). Drop the binaries
anywhere on `$PATH`.

## Shell completions and man pages

Every archive ships completions for bash, zsh, fish, PowerShell, and
Nushell under `completions/`, and man pages under `man/`. The installer
scripts only place the **binaries**; the completions and man pages are
bundled for you to install where your shell expects them:

```sh
# bash
install -Dm644 completions/aozora-fmt.bash ~/.local/share/bash-completion/completions/aozora-fmt
install -Dm644 completions/aozora-lsp.bash ~/.local/share/bash-completion/completions/aozora-lsp

# zsh (a directory on your $fpath, before `compinit`)
install -Dm644 completions/_aozora-fmt ~/.zfunc/_aozora-fmt

# fish
install -Dm644 completions/aozora-fmt.fish ~/.config/fish/completions/aozora-fmt.fish

# man pages
install -Dm644 man/aozora-fmt.1 ~/.local/share/man/man1/aozora-fmt.1
install -Dm644 man/aozora-lsp.1 ~/.local/share/man/man1/aozora-lsp.1
```

PowerShell users dot-source `completions/_aozora-fmt.ps1` from their
profile; Nushell users `source` `completions/aozora-fmt.nu`.

## From source

Requires the Rust toolchain pinned in [`rust-toolchain.toml`](https://github.com/P4suta/aozora-tools/blob/main/rust-toolchain.toml)
(currently 1.95.0).

```sh
# Both binaries:
cargo install --git https://github.com/P4suta/aozora-tools --locked aozora-fmt
cargo install --git https://github.com/P4suta/aozora-tools --locked aozora-lsp

# Or pin to a specific tag:
cargo install --git https://github.com/P4suta/aozora-tools --tag v0.1.3 --locked aozora-fmt
```

`--locked` makes cargo honour the workspace `Cargo.lock`, which pins
the [`aozora`](https://github.com/P4suta/aozora) parser at a known-good
revision. Skip it only if you are intentionally floating to a newer parser.

## Verify the install

```sh
aozora-fmt --version
aozora-lsp --version
```

Both binaries print their semver and the embedded `aozora` parser version.
