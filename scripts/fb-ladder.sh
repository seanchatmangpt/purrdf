#!/bin/bash
# fb-ladder.sh — the tiered feedback-loop SLA ladder.
#
#   T1 static     : toolchain-pin + no-std + layers Python gates (bounded)
#   T2 oracles    : crates/branchless/oracles pytest      (bounded, ~0.6s)
#   T3 leaf-probe : cargo check -p purrdf-branchless      (lease outer, bounded inner)
#
# Every rung emits the one-home receipt line (scripts/fb-receipt.sh):
#   FB <tier> <name> <count> <ms> <PASS|FAIL|SKIP:<reason>>
# Short-circuited rungs emit SKIP:tier<N>-failed (loud, counted). A rung that
# emits no receipt at all makes the verdict UNKNOWN (fail-closed on silent
# skip). Per-tier and total wall-clock are checked against the downward-only
# ratchet in scripts/fb-budgets.json; the aggregate receipts are appended to
# scripts/fb-receipts.jsonl via scripts/fb-court.py and the drift court runs
# at the end.
#
# Usage: fb-ladder.sh [--tier 1|2|3] [--lane <name>] [--no-court]
# No flag extends the 5s bounded lease anywhere; the ladder adds none.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

source scripts/fb-receipt.sh

LANE="${PURRDF_FB_LANE:-fb-ladder}"
ONLY_TIER=0
NO_COURT=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --tier) ONLY_TIER="${2:?--tier needs 1|2|3}"; shift 2 ;;
        --lane) LANE="${2:?--lane needs a name}"; shift 2 ;;
        --no-court) NO_COURT=1; shift ;;
        -h|--help) printf 'usage: fb-ladder.sh [--tier 1|2|3] [--lane <name>] [--no-court]\n' >&2; exit 0 ;;
        *) printf 'fb-ladder: unknown arg %s\n' "$1" >&2; exit 64 ;;
    esac
done
case "$LANE" in
    *..*|*[!A-Za-z0-9._-]*) printf 'fb-ladder: bad lane name %s\n' "$LANE" >&2; exit 64 ;;
esac

PLANNED_RUNGS=0
EMITTED_RUNGS=0
FAIL=0
declare -a PLAN=()
[ "$ONLY_TIER" -ne 2 ] && [ "$ONLY_TIER" -ne 3 ] && PLAN+=("t1") || true
if [ "$ONLY_TIER" -eq 0 ] || [ "$ONLY_TIER" -eq 2 ]; then PLAN+=("t2"); fi
if [ "$ONLY_TIER" -eq 0 ] || [ "$ONLY_TIER" -eq 3 ]; then PLAN+=("t3"); fi
PLANNED_RUNGS=${#PLAN[@]}

T0=$(fb_time)

# Rungs are inlined below; each emits its own receipt via fb_emit and
# records per-tier wall-clock for the budget tripwires and the court.

# tier_budget <tier-key> — echo budget_s from the ratchet (empty if absent).
tier_budget() {
    python3 - "$1" <<'PYEOF'
import json, sys
cfg = json.load(open("scripts/fb-budgets.json"))
tier = sys.argv[1]
entry = cfg.get(tier)
print(entry["budget_s"] if entry else "")
PYEOF
}

run_t1() {
    # The agent-iteration rung: static gates over the tree's structural
    # invariants. bounded-fmt is deliberately NOT here — its scope is the
    # whole working-tree delta vs HEAD, which legitimately includes long-
    # lived feature waves; it runs as the pre-commit hook (make hooks),
    # never in the 5s-class ladder (measured 2.0s over a 50-file wave).
    local gates=(check-toolchain-pin.py check-no-std.py check-layers.py)
    local n=0 start ms
    start=$(fb_time)
    for gate in "${gates[@]}"; do
        bash scripts/bounded python3 "scripts/$gate" >/dev/null 2>&1 || { FAIL=1; }
        n=$((n + 1))
    done
    ms=$(( $(fb_time) - start ))
    if [ "$FAIL" -ne 0 ]; then
        fb_emit t1 static "$n" "$ms" FAIL
    else
        fb_emit t1 static "$n" "$ms" PASS
    fi
    EMITTED_RUNGS=$((EMITTED_RUNGS + 1))
    T1_MS=$ms
    LAST_TIER_VERDICT=$FAIL
    [ "$FAIL" -eq 0 ] || return 1
    return 0
}

run_t2() {
    local start ms
    start=$(fb_time)
    bash scripts/bounded python3 -m pytest crates/branchless/oracles -q >/dev/null 2>&1
    local rc=$?
    ms=$(( $(fb_time) - start ))
    if [ "$rc" -ne 0 ]; then FAIL=1; fb_emit t2 oracles 7296 "$ms" FAIL
    else fb_emit t2 oracles 7296 "$ms" PASS; fi
    EMITTED_RUNGS=$((EMITTED_RUNGS + 1))
    T2_MS=$ms
    LAST_TIER_VERDICT=$rc
    [ "$rc" -eq 0 ] || return 1
    return 0
}

run_t3() {
    local start ms
    start=$(fb_time)
    # Lease OUTER, bounded INNER: the 500ms lease wait never eats cargo's 5s.
    # Env law: an ambient RUSTC_WRAPPER export must not turn the leaf probe
    # into an sccache hit-rate measurement. (Fast-lane follow-up.)
    env -u RUSTC_WRAPPER -u RUSTC_WRAPPER_BASE \
        bash scripts/lane-lease.sh exec --wait 500 "$LANE" -- \
        bash scripts/bounded cargo check -p purrdf-branchless --locked \
        >/dev/null 2>&1
    local rc=$?
    ms=$(( $(fb_time) - start ))
    if [ "$rc" -ne 0 ]; then FAIL=1; fb_emit t3 leaf-probe 1 "$ms" FAIL
    else fb_emit t3 leaf-probe 1 "$ms" PASS; fi
    EMITTED_RUNGS=$((EMITTED_RUNGS + 1))
    T3_MS=$ms
    LAST_TIER_VERDICT=$rc
    [ "$rc" -eq 0 ] || return 1
    return 0
}

emit_skips_for_remaining() {
    local seen=0
    for tier in "${PLAN[@]}"; do
        seen=$((seen + 1))
        if [ "$seen" -le "$EMITTED_RUNGS" ]; then continue; fi
        case "$tier" in
            t1) fb_emit t1 static 0 0 SKIP:unreached ;;
            t2) fb_emit t2 oracles 0 0 SKIP:t1-failed ;;
            t3) fb_emit t3 leaf-probe 0 0 SKIP:t2-failed ;;
        esac
        EMITTED_RUNGS=$((EMITTED_RUNGS + 1))
    done
}

T1_MS=0; T2_MS=0; T3_MS=0
LADDER_RC=0
for tier in "${PLAN[@]}"; do
    case "$tier" in
        t1) run_t1 || { LADDER_RC=1; break; } ;;
        t2) run_t2 || { LADDER_RC=1; break; } ;;
        t3) run_t3 || { LADDER_RC=1; break; } ;;
    esac
done
emit_skips_for_remaining

TOTAL_MS=$(( $(fb_time) - T0 ))

if [ "$EMITTED_RUNGS" -ne "$PLANNED_RUNGS" ]; then
    printf 'fb-ladder: VERDICT UNKNOWN (receipts=%s planned=%s total=%sms)\n' \
        "$EMITTED_RUNGS" "$PLANNED_RUNGS" "$TOTAL_MS"
    exit 1
fi

# Budget tripwires from the downward-only ratchet (immediate, per-tier).
for spec in "t1:$T1_MS" "t2:$T2_MS" "t3:$T3_MS"; do
    key="${spec%%:*}"; ms="${spec##*:}"
    [ "$ms" -eq 0 ] && continue
    budget_s=$(tier_budget "$key")
    [ -z "$budget_s" ] && continue
    budget_ms=$(printf '%s' "$budget_s" | awk '{printf "%d", $1*1000}')
    if [ "$ms" -gt "$budget_ms" ]; then
        printf 'fb-ladder: BUDGET-BREACH: %s took %sms (budget %sms) — probe wrong, not machine slow\n' \
            "$key" "$ms" "$budget_ms" >&2
        FAIL=1
        LADDER_RC=1
    fi
done

TOTAL_BUDGET_MS=$(printf '%s' "$(tier_budget total)" | awk '{printf "%d", $1*1000}')
if [ "$TOTAL_MS" -gt "$TOTAL_BUDGET_MS" ]; then
    printf 'fb-ladder: BUDGET-BREACH: total %sms exceeds %sms — probe wrong, not machine slow\n' \
        "$TOTAL_MS" "$TOTAL_BUDGET_MS" >&2
    FAIL=1
    LADDER_RC=1
fi

if [ "$FAIL" -eq 0 ]; then
    printf 'fb-ladder: VERDICT PASS (t1=%sms t2=%sms t3=%sms total=%sms)\n' \
        "$T1_MS" "$T2_MS" "$T3_MS" "$TOTAL_MS"
else
    printf 'fb-ladder: VERDICT FAIL (t1=%sms t2=%sms t3=%sms total=%sms)\n' \
        "$T1_MS" "$T2_MS" "$T3_MS" "$TOTAL_MS"
fi

if [ "$NO_COURT" -eq 0 ]; then
    python3 scripts/fb-court.py append --tier t1 --metric "$(awk -v m="$T1_MS" 'BEGIN{print m/1000}')" --verdict "$( [ "$T1_MS" -eq 0 ] && echo SKIP || { [ "$LADDER_RC" -ne 0 ] && echo FAIL || echo PASS; } )" >/dev/null 2>&1 || true
    python3 scripts/fb-court.py append --tier t2 --metric "$(awk -v m="$T2_MS" 'BEGIN{print m/1000}')" --verdict "$( [ "$T2_MS" -eq 0 ] && echo SKIP || { [ "$LADDER_RC" -ne 0 ] && echo FAIL || echo PASS; } )" >/dev/null 2>&1 || true
    python3 scripts/fb-court.py append --tier t3 --metric "$(awk -v m="$T3_MS" 'BEGIN{print m/1000}')" --verdict "$( [ "$T3_MS" -eq 0 ] && echo SKIP || { [ "$LADDER_RC" -ne 0 ] && echo FAIL || echo PASS; } )" >/dev/null 2>&1 || true
    python3 scripts/fb-court.py append --tier total --metric "$(awk -v m="$TOTAL_MS" 'BEGIN{print m/1000}')" --verdict "$( [ "$LADDER_RC" -eq 0 ] && echo PASS || echo FAIL )" >/dev/null 2>&1 || true
    if ! python3 scripts/fb-court.py check; then
        LADDER_RC=1
    fi
fi

exit "$LADDER_RC"
