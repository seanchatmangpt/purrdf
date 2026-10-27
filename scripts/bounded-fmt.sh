#!/bin/bash
# bounded-fmt.sh — chunked rustfmt over changed .rs files (parse-only, no
# compilation): batches of 5 files per rustfmt invocation so a single slow
# file can never hold the whole workspace format hostage. Every rustfmt
# invocation runs under the scripts/bounded 5s wall-clock lease (exit 124 on
# breach); wrapper output is suppressed — a run reports exactly ONE typed FB
# receipt, never per-batch chatter or a raw diff dump.
#
# File discovery (git present):
#   git diff --name-only --diff-filter=ACMR HEAD -- '*.rs'
#   git ls-files -o --exclude-standard -- '*.rs'   (untracked, not ignored)
# Fallback (git unavailable or not a repo): format the file paths given as
# argv instead.
#
# Receipt (scripts/fb-receipt.sh, tier t1 — one line per run):
#   FB t1 fmt <files> <ms> PASS|FAIL
#
# Exit: 0 iff every batch formatted (or --check'd) clean.
set -u

BATCH_SIZE=5
EDITION=2024
CHECK=0
if [ "${1:-}" = "--check" ]; then
    CHECK=1
    shift
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/fb-receipt.sh"
SCRIPT_START_MS=$(fb_time)

collect_changed() {
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        { git diff --name-only --diff-filter=ACMR HEAD -- '*.rs'
          git ls-files -o --exclude-standard -- '*.rs'; } | sort -u
    else
        # DIVERGENCE from typical CI formatters: argv fallback when git is
        # unavailable — the caller supplies the file list directly.
        printf '%s\n' "$@"
    fi
}

MAPFILE_CMD=()
if [ "$CHECK" -eq 1 ]; then
    MAPFILE_CMD=(--check)
fi

FILES=$(collect_changed "$@")

# Gather existing files into a batch array.
BATCH=()
NFILES=0
FAIL=0
flush_batch() {
    [ ${#BATCH[@]} -eq 0 ] && return 0
    # bash 3.2 + set -u: expanding an empty array is an unbound-variable
    # error, so guard the optional --check element.
    if bash "$SCRIPT_DIR/bounded" rustfmt --edition "$EDITION" \
        ${MAPFILE_CMD[@]+"${MAPFILE_CMD[@]}"} "${BATCH[@]}" >/dev/null 2>&1; then
        rc=0
    else
        rc=$?
    fi
    if [ "$rc" -ne 0 ]; then
        FAIL=1
    fi
    BATCH=()
}

while IFS= read -r f; do
    [ -f "$f" ] || continue
    BATCH+=("$f")
    NFILES=$((NFILES + 1))
    if [ ${#BATCH[@]} -ge "$BATCH_SIZE" ]; then
        flush_batch
    fi
done <<EOF
$FILES
EOF
flush_batch

TOTAL_MS=$(( $(fb_time) - SCRIPT_START_MS ))
if [ "$FAIL" -ne 0 ]; then
    fb_emit 1 fmt "$NFILES" "$TOTAL_MS" FAIL
    exit 1
fi
fb_emit 1 fmt "$NFILES" "$TOTAL_MS" PASS
exit 0
