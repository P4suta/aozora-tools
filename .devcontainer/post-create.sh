#!/usr/bin/env bash
# Wire up THIS checkout: git hooks, JS deps, warm cargo cache. Mirrors the
# second half of `just bootstrap` plus the lefthook post-merge warmups.
set -euo pipefail

# shellcheck disable=SC1091
. "$HOME/.cargo/env"
eval "$("$HOME/.local/bin/mise" activate bash)"

# Hand the `target` named volume to the workspace user. Unlike the
# ~/.cargo caches (pre-created and chowned in the Dockerfile), target's
# mount point lives under the runtime workspace bind-mount, so Docker
# can't seed its ownership from the image and creates the fresh volume
# root-owned. Fix it once here so `cargo build` can write to it.
if [ -d target ] && [ ! -w target ]; then
    sudo chown "$(id -u):$(id -g)" target || true
fi

# Git hooks. NOTE: jj colocated repos bypass git hooks; the pre-push gate
# still runs for git users (see contrib/dev.md).
lefthook install

# VS Code extension deps.
(cd editors/vscode && bun install --frozen-lockfile)

# Warm the cargo cache so the first inner-loop build is fast.
cargo fetch --quiet || true

# Final readiness report (advisory; never fails the create step).
just doctor || true
