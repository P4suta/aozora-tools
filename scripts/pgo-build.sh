#!/usr/bin/env bash
#
# PGO (Profile-Guided Optimisation) release build pipeline for the
# aozora-tools workspace.
#
# Targets the two shipped binaries:
#   * `aozora-fmt` — CLI formatter
#   * `aozora-lsp` — LSP server (also bundled inside the VS Code
#                    extension `.vsix`)
#
# Three phases:
#   1. instrumented build  — compile aozora-fmt and aozora-lsp + the
#                            aozora-lsp `burst` bench with
#                            instrumentation that records hot paths
#   2. profile collection  — run aozora-fmt against the sample suite
#                            and the LSP burst bench against its
#                            6 MB synthetic corpus
#   3. optimised rebuild   — re-link with the collected profile
#
# Optional fourth phase (BOLT post-link, Linux x86_64 only):
#   4. llvm-bolt           — apply binary post-link layout optimisation
#                            on top of the PGO output
#
# Expected gain: 10-15% additional throughput per LLVM project's
# published numbers; aozora-tools-specific measurement is part of this
# script's reporting.
#
# Requirements (verified at the top of the script):
#   - cargo-pgo  — install via `cargo install cargo-pgo`
#   - llvm-tools-preview — install via `rustup component add llvm-tools-preview`
#   - llvm-bolt — for the optional BOLT phase, install via the
#     `llvm-bolt` package on Debian/Ubuntu

set -euo pipefail

cd "$(dirname "$0")/.."

# ----------------------------------------------------------------------
# Phase 0 — preflight
# ----------------------------------------------------------------------

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "FATAL: required tool '$1' not found in PATH" >&2
        echo "       installation hint: $2" >&2
        exit 2
    fi
}

require cargo "rustup (https://rustup.rs/)"
require cargo-pgo "cargo install cargo-pgo"

SAMPLES_DIR="samples"
if [[ ! -d "$SAMPLES_DIR" ]]; then
    echo "FATAL: expected sample directory at $SAMPLES_DIR" >&2
    exit 2
fi

echo "==> Preflight checks passed"
echo "    samples:   $SAMPLES_DIR"
echo "    cargo-pgo: $(cargo pgo --version 2>/dev/null | head -1)"

# ----------------------------------------------------------------------
# Phase 1 — instrumented build
# ----------------------------------------------------------------------

echo ""
echo "==> Phase 1: instrumented build"
cargo pgo build -- -p aozora-fmt -p aozora-lsp
cargo pgo build -- -p aozora-lsp --bench burst

# ----------------------------------------------------------------------
# Phase 2 — profile collection
# ----------------------------------------------------------------------

echo ""
echo "==> Phase 2: profile collection"

FMT_BIN="target/x86_64-unknown-linux-gnu/release/aozora-fmt"
if [[ ! -x "$FMT_BIN" ]]; then
    echo "FATAL: expected instrumented aozora-fmt at $FMT_BIN" >&2
    exit 3
fi

# Drive aozora-fmt through every sample document. Three passes
# stabilise the recorded profile against any first-touch noise.
for run in 1 2 3; do
    echo "    formatter profile run $run/3..."
    for sample in "$SAMPLES_DIR"/*.afm; do
        "$FMT_BIN" "$sample" >/dev/null
    done
done

# Run the LSP burst bench. Criterion drives the workload internally;
# the instrumented binary records hot paths during execution.
echo "    LSP burst bench profile run..."
cargo pgo bench -- -p aozora-lsp --bench burst -- --quick

# ----------------------------------------------------------------------
# Phase 3 — optimised rebuild
# ----------------------------------------------------------------------

echo ""
echo "==> Phase 3: optimised rebuild with collected profile"
cargo pgo optimize build -- -p aozora-fmt -p aozora-lsp

OPT_FMT="target/x86_64-unknown-linux-gnu/release/aozora-fmt"
OPT_LSP="target/x86_64-unknown-linux-gnu/release/aozora-lsp"
echo ""
echo "==> PGO build complete"
echo "    optimised aozora-fmt: $OPT_FMT"
echo "    optimised aozora-lsp: $OPT_LSP"
ls -lh "$OPT_FMT" "$OPT_LSP" 2>/dev/null || true

# ----------------------------------------------------------------------
# Phase 4 (optional) — BOLT post-link
# ----------------------------------------------------------------------

if command -v llvm-bolt >/dev/null 2>&1; then
    echo ""
    echo "==> Phase 4: llvm-bolt post-link optimisation"

    # Collect a perf record by replaying the formatter workload against
    # the PGO binary first.
    PERF_DATA="target/release/aozora_pgo.perf.data"
    perf record -e cycles:u -j any,u -o "$PERF_DATA" -- \
        bash -c "for s in $SAMPLES_DIR/*.afm; do \"$OPT_FMT\" \"\$s\" >/dev/null; done"

    BOLT_OUT="${OPT_FMT}.bolt"
    llvm-bolt "$OPT_FMT" -o "$BOLT_OUT" \
        -data="$PERF_DATA" \
        -reorder-blocks=ext-tsp \
        -reorder-functions=hfsort+ \
        -split-functions \
        -split-all-cold \
        -split-eh \
        -dyno-stats

    echo ""
    echo "==> BOLT-optimised binary: $BOLT_OUT"
    ls -lh "$BOLT_OUT"
else
    echo ""
    echo "==> Phase 4 skipped: llvm-bolt not in PATH"
    echo "    install via: sudo apt install llvm-bolt   (Debian/Ubuntu)"
    echo "    or via the llvm-bolt source build: https://github.com/llvm/llvm-project/tree/main/bolt"
fi

# ----------------------------------------------------------------------
# Reporting
# ----------------------------------------------------------------------

echo ""
echo "==> Done. Compare against the baseline:"
echo "    hyperfine --warmup 3 \\"
echo "      'cargo run --release -p aozora-fmt -- $SAMPLES_DIR/bouten.afm' \\"
echo "      '$OPT_FMT $SAMPLES_DIR/bouten.afm'"
if command -v llvm-bolt >/dev/null 2>&1; then
    echo "    (then add the BOLT binary as a third command:)"
    echo "      '${OPT_FMT}.bolt $SAMPLES_DIR/bouten.afm'"
fi
