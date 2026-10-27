#!/bin/bash
# fb-receipt.sh — THE one home for the feedback-SLA receipt grammar.
#
# Grammar (exactly 6 fields, space-separated, stdout):
#   FB <tier> <name> <count> <ms> <status>
#     tier   : t1 | t2 | t3   (static / oracle / leaf probe)
#     name   : [a-z0-9-]+
#     count  : non-negative integer (cases, files, or probes)
#     ms     : non-negative integer (wall-clock milliseconds)
#     status : PASS | FAIL | TIMEOUT | exit-<n> | SKIP:<reason-token>
#     (TIMEOUT and exit-<n> exist because scripts/bounded — a sanctioned
#      emitter — reports its lease verdicts with that fidelity; both count
#      as FAIL for aggregation.)
#
# helpers-ledger doctrine: one home per job. Any other file in the repo that
# emits a line starting with "FB " is a second body and a census violation
# (rule:fb-receipt-format). Sanctioned emitters, exhaustively:
#   - scripts/fb-receipt.py (Python gates cannot source bash — layering)
#   - scripts/bounded (coreutils path sources this lib; its perl fallback
#     execs, so it printfs the identical grammar inside the interpreter)
#
# Sourcing contract: safe under `set -euo pipefail`; emits nothing on source.

# fb_time — milliseconds since the epoch, integer.
fb_time() {
    if [ -n "${EPOCHREALTIME:-}" ]; then
        printf '%s\n' "${EPOCHREALTIME%%.*}${EPOCHREALTIME#*.}" | cut -c1-13
    else
        printf '%s\n' "$(( $(date +%s%N) / 1000000 ))"
    fi
}

# fb_emit <tier> <name> <count> <ms> <status> — print one receipt line.
# Normalizes bare "1|2|3" tiers to t1|t2|t3 so callers never guess wrong.
fb_emit() {
    local tier="$1" name="$2" count="$3" ms="$4" status="$5"
    case "$tier" in
        1|t1) tier=t1 ;;
        2|t2) tier=t2 ;;
        3|t3) tier=t3 ;;
        *) printf 'fb-receipt: bad tier %s\n' "$tier" >&2; return 2 ;;
    esac
    printf 'FB %s %s %s %s %s\n' "$tier" "$name" "$count" "$ms" "$status"
}

# fb_validate — validate one receipt line read from stdin. rc 0 = well-formed.
fb_validate() {
    local line
    line="$(cat)"
    printf '%s\n' "$line" | grep -qE '^FB t[123] [a-z0-9-]+ [0-9]+ [0-9]+ (PASS|FAIL|TIMEOUT|SKIP:[a-z0-9-]+|exit-[0-9]+)$'
}

# fb_append_jsonl <file> <single-line-json> — flock-locked, atomic single-line
# append. Never truncates; never rewrites existing content.
fb_append_jsonl() {
    local file="$1" line="$2" lock="${3:-${file}.lock}"
    (
        flock -x 9
        printf '%s\n' "$line" >>"$file"
    ) 9>"$lock"
}
