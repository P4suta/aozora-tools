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
`LICENSE-MIT`, and `README.md`. Drop the binaries anywhere on `$PATH`.

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
tag. Skip it only if you are intentionally floating to a newer parser.

## Verify the install

```sh
aozora-fmt --version
aozora-lsp --version
```

Both binaries print their semver and the embedded `aozora` parser tag.
