# aozora-tools developer recipes.
#
# Host toolchain (no Docker) — mirrors the recipe *surface* of the
# upstream aozora repo's Justfile so muscle memory carries across the
# sibling repos. `just` is optional: every recipe is a thin wrapper over
# cargo / xtask you can also run by hand.

set shell := ["bash", "-c"]

# Default: list recipes.
default:
    @just --list

# --- core gates --------------------------------------------------------------

# Test suite: workspace tests (shuttle + fuzz-regression replay) + doctests.
test *ARGS:
    cargo nextest run --workspace --all-targets --features aozora-lsp/shuttle-tests {{ARGS}}
    cargo test --doc --workspace

# Format the Rust workspace + the VS Code extension sources.
fmt:
    cargo fmt --all
    cd editors/vscode && bun run format

# Check formatting without writing (CI parity).
fmt-check:
    cargo fmt --all -- --check

# Clippy across all targets + features, warnings denied.
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build rustdoc with intra-doc-link breakage denied.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items

# Supply-chain audit: licenses, advisories, bans, sources.
deny:
    cargo deny check

# Coverage gate (delegates to xtask; same thresholds as CI).
cov *ARGS:
    cargo run -p aozora-tools-xtask -- coverage {{ARGS}}

# VS Code extension checks (biome + tsc).
vscode-check:
    cd editors/vscode && bun install && bun run check

# The full local gate — mirrors CI. Run before pushing.
ci: fmt-check clippy test doc deny
    @echo "ci: all gates passed"

# --- fuzzing -----------------------------------------------------------------
#
# cargo-fuzz harnesses live under `crates/<crate>/fuzz/` as nightly-only
# sub-crates *outside* the main workspace (their own `[workspace]`), so a
# plain `cargo build --workspace` never pulls in libfuzzer-sys. Targets
# currently registered:
#
#   aozora-fmt / format_idempotent  — format_source never panics and is a
#                                     fixed point: format(format(x)) == format(x)
#   aozora-lsp / edit_pipeline      — the byte/UTF-16 edit splice +
#                                     position round-trip never panic
#
# Workflow (mirrors the upstream aozora / afm fuzz-triage loop):
#
#   1. `just fuzz-quick CRATE TARGET`    (60 s)  — inner-loop smoke
#   2. `just fuzz-deep  CRATE TARGET`    (5 min) — release pre-flight
#   3. `just fuzz-marathon CRATE TARGET` (15 min)— strongest soak
#   4. On a crash, `just fuzz-triage CRATE TARGET` replays every artifact
#      under crates/<crate>/fuzz/artifacts/<target>/ and prints the panic.
#   5. `just fuzz-promote CRATE TARGET ARTIFACT` lifts a crash input into
#      crates/<crate>/tests/fuzz_regressions/<target>/ so the stable
#      `tests/fuzz_regressions.rs` test replays it on every `just test`
#      — no nightly required for the regression case.
#   6. `just fuzz-status` is the at-a-glance pending-crash vs pinned-
#      regression count per target.
#
# See the handbook (contrib/fuzzing) for the long-form description.

# Run a fuzz target with arbitrary args (escape hatch).
fuzz CRATE *ARGS:
    cd crates/{{CRATE}}/fuzz && cargo +nightly fuzz run {{ARGS}}

# 60-second smoke fuzz — fits a development inner loop.
fuzz-quick CRATE TARGET:
    cd crates/{{CRATE}}/fuzz && cargo +nightly fuzz run {{TARGET}} -- -max_total_time=60

# 5-minute deep fuzz — the gate to clear before tagging a release.
fuzz-deep CRATE TARGET:
    cd crates/{{CRATE}}/fuzz && cargo +nightly fuzz run {{TARGET}} -- -max_total_time=300

# 15-minute marathon fuzz — strongest single-target soak.
fuzz-marathon CRATE TARGET:
    cd crates/{{CRATE}}/fuzz && cargo +nightly fuzz run {{TARGET}} -- -max_total_time=900

# Run every registered target for 60 s each.
fuzz-all-quick:
    just fuzz-quick aozora-fmt format_idempotent
    just fuzz-quick aozora-lsp edit_pipeline

# Run every registered target for 5 min each — the release pre-flight.
fuzz-all-deep:
    just fuzz-deep aozora-fmt format_idempotent
    just fuzz-deep aozora-lsp edit_pipeline

# Replay all crash artifacts for a target; exit status = number still crashing.
fuzz-triage CRATE TARGET:
    #!/usr/bin/env bash
    set -uo pipefail
    crate="{{CRATE}}"; target="{{TARGET}}"
    art_dir="crates/${crate}/fuzz/artifacts/${target}"
    if [[ ! -d "$art_dir" ]] || [[ -z "$(ls -A "$art_dir" 2>/dev/null)" ]]; then
        echo "fuzz-triage: no artifacts for ${crate} / ${target}"
        exit 0
    fi
    failed=0
    for art in "$art_dir"/*; do
        [[ -f "$art" ]] || continue
        rel="${art#crates/${crate}/fuzz/}"
        if ! (cd "crates/${crate}/fuzz" && cargo +nightly fuzz run "$target" "$rel" >/dev/null 2>&1); then
            echo "CRASH: $art"
            failed=$((failed + 1))
        fi
    done
    if [[ "$failed" -gt 0 ]]; then
        echo "fuzz-triage: ${failed} artifact(s) still crash" >&2
        exit "$failed"
    fi
    echo "fuzz-triage: every artifact replays cleanly"

# Promote a fuzz artifact into the permanent stable regression set so
# tests/fuzz_regressions.rs pins it on every `just test` (no nightly).
fuzz-promote CRATE TARGET ARTIFACT:
    #!/usr/bin/env bash
    set -euo pipefail
    crate="{{CRATE}}"; target="{{TARGET}}"; art="{{ARTIFACT}}"
    dest_dir="crates/${crate}/tests/fuzz_regressions/${target}"
    mkdir -p "$dest_dir"
    cp "$art" "$dest_dir/$(basename "$art")"
    echo "promoted $(basename "$art") -> $dest_dir/"
    echo "commit it; tests/fuzz_regressions.rs now replays it on every run."

# At-a-glance pending-crash vs pinned-regression count per target.
fuzz-status:
    #!/usr/bin/env bash
    set -uo pipefail
    for fuzz_dir in crates/*/fuzz; do
        [[ -d "$fuzz_dir" ]] || continue
        crate="$(basename "$(dirname "$fuzz_dir")")"
        for tgt_dir in "$fuzz_dir"/fuzz_targets/*.rs; do
            [[ -f "$tgt_dir" ]] || continue
            target="$(basename "$tgt_dir" .rs)"
            pending=$(ls -A "crates/${crate}/fuzz/artifacts/${target}" 2>/dev/null | wc -l)
            pinned=$(ls -A "crates/${crate}/tests/fuzz_regressions/${target}" 2>/dev/null | wc -l)
            printf '%-14s %-20s pending=%s pinned=%s\n' "$crate" "$target" "$pending" "$pinned"
        done
    done
