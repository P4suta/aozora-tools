#!/usr/bin/env bash
# Keep Codespaces prebuilds warm: fetch cargo + JS deps. Runs in the
# updateContent phase (after onCreate, before postCreate), where a bare
# shell has neither cargo nor the mise shims on PATH yet — so source the
# toolchains the onCreate phase just installed. Best-effort: never fail
# the create.
set -uo pipefail

# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
[ -x "$HOME/.local/bin/mise" ] && eval "$("$HOME/.local/bin/mise" activate bash)"

cargo fetch --quiet || true
(cd editors/vscode && bun install --frozen-lockfile) || true
