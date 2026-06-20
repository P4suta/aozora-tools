#!/usr/bin/env bash
# Provision the pinned dev-tool set (prebuild-eligible phase).
#
# Rust comes from rustup (rust-toolchain.toml is the single pin);
# everything else comes from mise. We pass MISE_DISABLE_TOOLS=rust so mise
# never installs a competing rustc (devcontainer.json sets it for shells
# too; we set it inline here so this is correct regardless of when
# remoteEnv is applied).
set -euo pipefail

# 1. rustup + the pinned channel (auto-reads rust-toolchain.toml).
if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --no-modify-path
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
# Materialise the pinned toolchain + components now (not lazily on first
# cargo invocation) so Codespaces prebuilds bake it in.
rustup show

# 2. cargo-binstall so mise pulls the cargo:* tools (cargo-nextest,
#    cargo-deny, cargo-llvm-cov) as prebuilt binaries instead of compiling
#    them from source — minutes faster and the difference between a snappy
#    and a glacial first container build.
if ! command -v cargo-binstall >/dev/null 2>&1; then
    curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
fi

# 3. mise + the rest of the manifest (just, lefthook, typos, committed,
#    actionlint, bun, cargo-nextest, cargo-deny, cargo-llvm-cov).
if ! command -v mise >/dev/null 2>&1; then
    curl https://mise.run | sh
fi
eval "$("$HOME/.local/bin/mise" activate bash)"
MISE_CARGO_BINSTALL=true MISE_DISABLE_TOOLS=rust mise install
