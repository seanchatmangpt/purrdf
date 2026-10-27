#!/usr/bin/env bash
# sccache-fast.sh — opt-in sccache wrapper for fast lanes on this machine.
#
# Usage: scripts/sccache-fast.sh <command...>
#   e.g. scripts/sccache-fast.sh scripts/run_benches_fast.py run ...
#
# Opt-in ONLY: nothing in the repo default environment changes. The wrapper
# exports sccache env vars in this process (and children) only — no Cargo.toml,
# no .cargo/config.toml, no shell profile is touched. In-flight runners are
# unaffected.
#
# Env surface note (recorded 2026-10-27, sccache 0.16.0):
#   - RUSTC_WRAPPER=sccache: the universally compatible surface; works on
#     stable and nightly rustc, all cargo versions. Downside: changing the
#     wrapper invalidates cargo's fingerprint of the compiler ("wrapper
#     invalidation"), so toggling it mid-tree forces rebuilds.
#   - CARGO_WRAPPER=[pkg:]sccache: newer cargo surface that avoids
#     wrapper-invalidation semantics (cargo invokes sccache as a job-object
#     wrapper, not via RUSTC_WRAPPER), but requires a sufficiently new cargo.
#   Recommendation: RUSTC_WRAPPER for compatibility; it is what this script
#   sets. Override to CARGO_WRAPPER by exporting it before invoking — this
#   script does not clear caller-set variables it does not own.

set -euo pipefail

SCCACHE_BIN="/opt/homebrew/bin/sccache"
if [[ ! -x "$SCCACHE_BIN" ]]; then
    # Fallback to PATH lookup before failing.
    SCCACHE_BIN="$(command -v sccache 2>/dev/null || true)"
fi
if [[ -z "$SCCACHE_BIN" || ! -x "$SCCACHE_BIN" ]]; then
    echo "SCCACHE-FAST[REFUSED]: sccache not found at /opt/homebrew/bin/sccache or on PATH." >&2
    echo "  Install: brew install sccache   (or SCCACHE_BIN=<path> scripts/sccache-fast.sh ...)" >&2
    exit 1
fi
# Honor a caller-provided SCCACHE_BIN by re-resolving.
if [[ -n "${SCCACHE_BIN_OVERRIDE:-}" && -x "$SCCACHE_BIN_OVERRIDE" ]]; then
    SCCACHE_BIN="$SCCACHE_BIN_OVERRIDE"
fi

export RUSTC_WRAPPER="$SCCACHE_BIN"
export SCCACHE_DIR="${SCCACHE_DIR:-/tmp/purrdf-sccache}"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${FAST_JOBS:-8}}"

exec "$@"
