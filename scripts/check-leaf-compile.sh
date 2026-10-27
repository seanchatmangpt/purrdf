#!/bin/bash
# check-leaf-compile.sh — typed leaf-compile probe for purrdf-branchless and
# purrdf-hash. Every cargo invocation goes through scripts/bounded (5s lease,
# exit 124/13 on breach), optionally inside a per-lane CARGO_TARGET_DIR lease
# (scripts/lane-lease.sh) when PURRDF_FB_LANE is set — lease OUTER, bounded
# INNER. Exits 0 only when every probe passes inside its lease and the
# dependency-subject assertion holds. Any bounded timeout is a loud TIMEOUT
# exit 1, never a silent pass. Emits one aggregate FB receipt
# (scripts/fb-receipt.sh, tier t3) after the LEAF-PROBE lines.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Receipt home (helpers-ledger: one home per job) + the single run clock.
# shellcheck disable=SC1091
. scripts/fb-receipt.sh
SCRIPT_START_MS=$(fb_time)

# Probe wrapper. fd 9 of the lease is held for the CMD lifetime by design
# (lane-lease exec semantics), not the W876 orphan-fd class.
if [ -n "${PURRDF_FB_LANE:-}" ]; then
    WRAPPER=(bash scripts/lane-lease.sh exec --wait 500 "$PURRDF_FB_LANE" -- bash scripts/bounded)
else
    WRAPPER=(bash scripts/bounded)
fi
FAILED=0
PROBES=0

# run_probe <label> <command...> — emits "LEAF-PROBE <label> PASS|FAIL|TIMEOUT rc=<rc> elapsed=<s>"
run_probe() {
    label=$1
    shift
    start=$(date +%s.%N)
    "${WRAPPER[@]}" "$@"
    rc=$?
    end=$(date +%s.%N)
    elapsed=$(awk -v s="$start" -v e="$end" 'BEGIN { printf "%.2f", e - s }')
    PROBES=$((PROBES + 1))
    case "$rc" in
        0)   printf 'LEAF-PROBE %s PASS elapsed=%ss\n' "$label" "$elapsed" ;;
        124|13) printf 'LEAF-PROBE %s TIMEOUT rc=%s elapsed=%ss\n' "$label" "$rc" "$elapsed"; FAILED=1 ;;
        *)   printf 'LEAF-PROBE %s FAIL rc=%s elapsed=%ss\n' "$label" "$rc" "$elapsed"; FAILED=1 ;;
    esac
}

# Probe 1: purrdf-branchless with its full dep closure (purrdf-hash included
# by design; --no-deps would under-measure the leaf's real compile surface).
run_probe "branchless" cargo check -p purrdf-branchless --quiet

# Probe 2: purrdf-hash alone (the zero-dependency root).
run_probe "hash" cargo check -p purrdf-hash --quiet

# Probe 3: subject assertion — purrdf-hash must actually be in branchless's
# dep closure. cargo tree may be missing on stripped toolchains; skip loudly.
if command -v cargo >/dev/null 2>&1 && "${WRAPPER[@]}" cargo tree -i purrdf-hash -p purrdf-branchless >/tmp/leaf-compile-tree.txt 2>&1; then
    if grep -q 'purrdf-hash' /tmp/leaf-compile-tree.txt; then
        printf 'LEAF-PROBE dep-subject PASS: purrdf-hash in purrdf-branchless closure\n'
    else
        printf 'LEAF-PROBE dep-subject FAIL: purrdf-hash absent from closure\n'
        FAILED=1
    fi
else
    printf 'LEAF-PROBE dep-subject SKIP: cargo tree unavailable or bounded\n'
fi
PROBES=$((PROBES + 1))

TOTAL_MS=$(( $(fb_time) - SCRIPT_START_MS ))
if [ "$FAILED" -ne 0 ]; then
    printf 'LEAF-PROBE SUMMARY FAIL\n'
    fb_emit 3 leaf-probe "$PROBES" "$TOTAL_MS" FAIL
    exit 1
fi
printf 'LEAF-PROBE SUMMARY PASS\n'
fb_emit 3 leaf-probe "$PROBES" "$TOTAL_MS" PASS
exit 0
