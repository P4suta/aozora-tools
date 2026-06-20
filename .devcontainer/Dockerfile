# Recommended onboarding image for aozora-tools.
#
# Thin Debian base + native build deps only. Rust (rustup) and the mise
# tool set are layered at create time by .devcontainer/on-create.sh, so
# rust-toolchain.toml stays the single Rust pin (no version baked here).
FROM mcr.microsoft.com/devcontainers/base:bookworm

# Native build deps:
#   build-essential, clang  -> C toolchain for crates/tree-sitter-aozora/parser.c
#   pkg-config              -> common sys-crate discovery
#   mold                    -> fast linker (RUSTFLAGS opts in; see devcontainer.json)
#   curl, ca-certificates   -> rustup + mise installers
#   git                     -> already present in the base image; kept explicit
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential \
      clang \
      pkg-config \
      mold \
      curl \
      ca-certificates \
      git \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

# Pre-create the cache mount points owned by the non-root user. The named
# volumes mounted here (see devcontainer.json) inherit this ownership when
# first created; without it Docker creates the parent dirs as root and
# rustup/mise/bun cannot write to ~/.cargo, ~/.bun, ~/.local.
RUN mkdir -p \
      /home/vscode/.cargo/registry \
      /home/vscode/.cargo/git \
      /home/vscode/.bun/install/cache \
      /home/vscode/.local/share/mise \
 && chown -R vscode:vscode /home/vscode/.cargo /home/vscode/.bun /home/vscode/.local
