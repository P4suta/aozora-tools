# Bundled .vsix release flow

Each VS Code extension release is a platform-specific `.vsix`
archive that contains the cross-compiled `aozora-lsp` binary at
`server/aozora-lsp(.exe)`. End users install one `.vsix` and get
both halves; no separate LSP install.

## Trigger

`.github/workflows/release-vscode.yml` fires on tags matching
`vscode-v*` (e.g. `vscode-v0.1.4`). Release tags for the Rust
binaries (`v0.1.4`) are independent — the VS Code extension follows
its own Marketplace cadence, bundling whatever `aozora-lsp` is
current at tag time.

## Matrix

| Target triple | Runner | `.vsix` platform |
|---|---|---|
| `x86_64-unknown-linux-gnu`     | `ubuntu-latest`  | `linux-x64`   |
| `x86_64-unknown-linux-musl`    | `ubuntu-latest`  | `alpine-x64`  (musl static) |
| `aarch64-apple-darwin`         | `macos-latest`   | `darwin-arm64` |
| `x86_64-apple-darwin`          | `macos-latest`   | `darwin-x64`   |
| `x86_64-pc-windows-msvc`       | `windows-latest` | `win32-x64`   |

The musl target enables Alpine Linux installs (the default Docker
base image many users run VS Code Server in).

## Build steps

For each matrix entry:

1. Install the Rust toolchain pinned in `rust-toolchain.toml` plus
   the matrix target.
2. `cargo build --release --target <triple> --package aozora-lsp`
   — the workspace `[profile.dist]` is **not** used here; the LSP
   bundles still want the full `[profile.release]` for runtime
   speed since the `.vsix` size is bandwidth-trivial compared to
   the editor itself.
3. `cd editors/vscode && bun run package` — `vsce` produces a
   `.vsix` with the extension JS + the LSP binary at
   `server/aozora-lsp(.exe)`.
4. Upload as a workflow artefact.

## Publish step

`publish-vscode` job, `needs: [build (...)]`, runs once per release:

1. Download all matrix `.vsix` artefacts.
2. `vsce publish --packagePath <each .vsix>` — uploads to the
   Marketplace.
3. `ovsx publish <each .vsix>` — uploads to
   [Open VSX](https://open-vsx.org/) (the Eclipse-Foundation
   alternative registry that Codium / Cursor / Eclipse Theia
   default to).

## Secrets

| Secret | Where to set | What for |
|---|---|---|
| `VSCE_TOKEN` | repo secrets | `vsce publish` Personal Access Token (Marketplace). |
| `OVSX_TOKEN` | repo secrets | `ovsx publish` Personal Access Token (Open VSX). |

Both are scoped to the publisher account `yasunobu-sakashita`. The
publish step is gated on tag pushes; manual `workflow_dispatch`
runs build but skip publish.

## Verifying a release

Marketplace and Open VSX both serve a download page after publish.
Spot-check by:

```sh
# Latest published version on Marketplace:
curl -s https://marketplace.visualstudio.com/_apis/public/gallery/publishers/yasunobu-sakashita/vsextensions/aozora/latest/vspackage \
  | tar -tjvf - 2>/dev/null | head -5

# Open VSX:
curl -s https://open-vsx.org/api/yasunobu-sakashita/aozora/latest \
  | jq '.version'
```

The `extension/server/aozora-lsp` entry in the unpacked `.vsix`
should match the LSP binary from the corresponding `v*` Release
on this repo.
