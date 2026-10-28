#!/bin/bash
# test-bounded-guard.sh — non-vacuity corpus for the 5s bounded-build law,
# covering four axes:
#   FIRE  — the PreToolUse hook (.claude/hooks/bounded_build_guard.py) must
#           refuse unbounded build/test commands (exit 2).
#   ALLOW — the hook must pass bounded/fast lawful commands (exit 0).
#   KILL  — the scripts/bounded wrapper must kill over-lease payloads at ~5s
#           with a typed BOUNDED-TIMEOUT marker, and must NOT interfere with
#           short payloads (positive control catches a LIMIT regression).
#   SELF-CHECK — a neutered hook copy must stop firing on `cargo test`, and
#           check_kill must reject a dead-guard signature; if the corpus
#           cannot detect a dead guard it fails VACUOUS.
# Dependencies owned by other lanes degrade LOUD as SKIP:<token>, never a
# silent pass and never a fake FAIL: SKIP:perl-124-pending (legacy kill rc),
# SKIP:diagnosis-string-pending, SKIP:receipt-log-pending,
# SKIP:selfcheck-anchor-pending, SKIP:selfcheck-copy-broken.
# Total budget: <= 10s wall clock (measured 8.9s: one ~5s kill + one 2s allow
# + ~2s of hook-spawn overhead across 43 probe cases).
set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOOK="$ROOT/.claude/hooks/bounded_build_guard.py"
WRAPPER="$ROOT/scripts/bounded"
RECEIPT_LOG="$ROOT/.claude/hooks/bounded-guard-receipts.log"
FAIL=0

feed_hook() {
    # $1 = hook path; $2 = command string; prints hook exit code
    printf '{"tool_name":"Bash","tool_input":{"command":%s}}' \
        "$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$2")" \
        | python3 "$1" >/dev/null 2>&1
    echo $?
}

elapsed_ok() {
    # $1 = value, $2 = min, $3 = max ("" max = unbounded)
    awk -v e="$1" -v lo="$2" -v hi="$3" 'BEGIN { exit !(e >= lo && (hi == "" || e <= hi)) }'
}

# check_kill <rc> <elapsed> <typed_present> [label]
# Lawful kill claim: rc is the kill signature (124 coreutils, or 137 SIGKILL
# race; 13 = legacy perl fallback -> SKIP:perl-124-pending), elapsed shows the
# 5s lease actually elapsed ([4.5, 8.0] — a sub-4.5s kill means the limit
# regressed, >8s overshoot means the grace path broke), and the typed
# BOUNDED-TIMEOUT marker is present. Anything else is VACUOUS. Used by the
# real probes below AND by the canned self-check mutant.
check_kill() {
    local rc="$1" elapsed="$2" typed="$3" label="${4:-kill probe}"
    if [ "$typed" != "present" ] && [ "$typed" != "absent" ]; then
        echo "VACUOUS: $label — typed_present arg must be present|absent, got '$typed'"
        return 1
    fi
    if [ "$rc" -eq 13 ]; then
        if [ "$typed" = "present" ] && elapsed_ok "$elapsed" 4.5 8.0; then
            echo "SKIP:perl-124-pending (kill via legacy rc=13, ${elapsed}s, typed) $label"
            return 0
        fi
        echo "VACUOUS: $label rc=13 (legacy perl kill) but typed=$typed elapsed=${elapsed}s (want typed + ~5s)"
        return 1
    fi
    if [ "$rc" -ne 124 ] && [ "$rc" -ne 137 ]; then
        echo "VACUOUS: $label rc=$rc elapsed=${elapsed}s typed=$typed — wrapper did NOT kill (want 124)"
        return 1
    fi
    if ! elapsed_ok "$elapsed" 4.5 8.0; then
        echo "VACUOUS: $label rc=$rc but elapsed=${elapsed}s outside [4.5,8.0]s (limit regression or broken grace)"
        return 1
    fi
    if [ "$typed" != "present" ]; then
        echo "VACUOUS: $label killed (rc=$rc, ${elapsed}s) but typed BOUNDED-TIMEOUT marker absent"
        return 1
    fi
    echo "PASS(kill): $label rc=$rc after ${elapsed}s, typed marker present"
    return 0
}

echo "== MUST-FIRE cases (hook exit 2 + block) =="
MUST_FIRE=(
    "cargo test"
    "cargo build"
    "cargo clippy"
    "cargo run"
    "rustc x.rs"
    "make test"
    "make check"
    "make pytest"
    "make wasm"
    "make conformance"
    "make"
    "cargo fmt"
    "true && cargo test"
    "cd bindings/python && cargo test"
    "RUSTFLAGS=-Dwarnings cargo clippy"
    "timeout -k 1 600 cargo test"
    "sh -c 'cargo test'"
    "sh -c 'cd /tmp && cargo test --release'"
    # Allowlist is not a wildcard: bare `bash <script>.sh` classifies the
    # script itself; only BOUNDED_SCRIPTS members are admitted.
    "bash scripts/some-random.sh"
    "bash scripts/not-in-allowlist.sh"
    "zsh -c 'cargo build'"
    "eval 'cargo test'"
    "env cargo test"
    "time cargo build"
    "xargs cargo test"
    "echo \$(cargo test)"
    "echo \`cargo test\`"
    "timeout 600 cargo test"
    "make unknown-target"
    "sh"
    "cargo fmt --foo"
)
for cmd in "${MUST_FIRE[@]}"; do
    rc=$(feed_hook "$HOOK" "$cmd")
    if [ "$rc" -eq 2 ]; then
        echo "PASS(fire): $cmd"
    else
        echo "VACUOUS: hook did not fire (rc=$rc, want 2): $cmd"
        FAIL=1
    fi
done

echo "== MUST-PASS cases (hook exit 0) =="
# Bench carve-out (operator 2026-10-27): serialized batch bench runs are
# outside the 5s agent-iteration law. Pinned so a CARGO_FAST refactor that
# drops "bench" flips these from allow to refuse (non-vacuous).
MUST_PASS=(
  "cargo bench -p purrdf-lex"
  "scripts/bounded cargo bench -p purrdf-lex"

    "python3 scripts/check-layers.py"
    "cargo fmt --check"
    "cargo metadata --format-version 1"
    "scripts/bounded cargo test"
    "timeout -k 1 5 cargo test"
    "git status"
    "ls"
    "rustfmt --edition 2024 --check crates/branchless/src/lib.rs"
    "scripts/bounded bash scripts/bounded-fmt.sh"
    "scripts/bounded bash scripts/bounded-fmt.sh --check"
    # .sh allowlist (hook BOUNDED_SCRIPTS): bare `bash <script>.sh` classifies
    # the script itself; these three are admitted, anything else .sh refuses.
    "bash scripts/bounded-fmt.sh"
    "bash scripts/bounded-fmt.sh --check"
    "bash scripts/lane-lease.sh list"
    "bash scripts/test-bounded-guard.sh --help"
    # make b-<crate>/bt-<crate> lease targets are admitted fast targets.
    "make b-purrdf-lex"
    "make bt-purrdf-core CARGO_TEST_FILTER=owned_term_walks"
    "timeout -k 1 5 make help"
    "scripts/bounded make fmt"
    "cd /tmp && cargo fmt --check"
)
for cmd in "${MUST_PASS[@]}"; do
    rc=$(feed_hook "$HOOK" "$cmd")
    if [ "$rc" -eq 0 ]; then
        echo "PASS(allow): $cmd"
    else
        echo "VACUOUS: hook refused a lawful command (rc=$rc, want 0): $cmd"
        FAIL=1
    fi
done

echo "== wrapper behavior =="
"$WRAPPER" true >/dev/null 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then
    echo "PASS(allow): scripts/bounded true -> rc 0"
else
    echo "VACUOUS: scripts/bounded true returned rc=$rc, want 0"
    FAIL=1
fi

echo "== positive control: short payload must run to completion =="
# Catches a LIMIT regression below 2s: if the lease shrank, sleep 2 dies
# (rc 124 + BOUNDED-TIMEOUT marker) instead of exiting 0.
START=$(python3 -c 'import time; print(time.time())')
ERR_OK=$(mktemp)
"$WRAPPER" sleep 2 >/dev/null 2>"$ERR_OK"
rc=$?
END=$(python3 -c 'import time; print(time.time())')
ELAPSED=$(python3 -c "print($END - $START)")
if [ "$rc" -eq 0 ] && elapsed_ok "$ELAPSED" 0 4.5 && ! grep -q "BOUNDED-TIMEOUT" "$ERR_OK"; then
    echo "PASS(allow): scripts/bounded sleep 2 -> rc 0 after ${ELAPSED}s, no BOUNDED-TIMEOUT"
else
    echo "VACUOUS: scripts/bounded sleep 2 rc=$rc elapsed=${ELAPSED}s (want rc 0, <4.5s, no marker) — LIMIT regression below 2s?"
    FAIL=1
fi
rm -f "$ERR_OK"

echo "== kill probe: over-lease payload must die at the 5s lease =="
START=$(python3 -c 'import time; print(time.time())')
ERRFILE=$(mktemp)
"$WRAPPER" sleep 6 >/dev/null 2>"$ERRFILE"
rc=$?
END=$(python3 -c 'import time; print(time.time())')
ELAPSED=$(python3 -c "print($END - $START)")
TYPED=absent
grep -q "BOUNDED-TIMEOUT\[5s\]: sleep 6" "$ERRFILE" && TYPED=present
if check_kill "$rc" "$ELAPSED" "$TYPED" "scripts/bounded sleep 6"; then
    :
else
    FAIL=1
fi
if grep -q "probe wrong, not machine slow" "$ERRFILE"; then
    echo "PASS(diagnosis): kill stderr names the failure class"
else
    echo "SKIP:diagnosis-string-pending (kill stderr lacks 'probe wrong, not machine slow')"
fi
rm -f "$ERRFILE"

echo "== self-check: check_kill must reject a dead-guard signature =="
MUT=$(check_kill 0 6.0 absent "canned-mutant(rc=0, 6.0s, typed=absent)")
if [ "$?" -ne 0 ]; then
    echo "PASS(self-check): check_kill rejected the mutant: $MUT"
else
    echo "VACUOUS: check_kill ACCEPTED canned mutant (rc=0, 6.0s, no typed marker) — kill assertion is decorative"
    FAIL=1
fi

echo "== self-check: neutered hook must stop firing =="
HOOKCOPY=$(mktemp /tmp/w3-neutered-hook.XXXXXX.py)
sed -e 's|^BLOCK_VERBS = .*|BLOCK_VERBS = set()  # NEUTERED-VERBS|' \
    -e 's|^BUILD_VERB = .*|BUILD_VERB = re.compile(r"NEUTERED-NEVER-MATCH")  # NEUTERED-BUILDVERB|' \
    "$HOOK" > "$HOOKCOPY"
if ! grep -q "NEUTERED-VERBS" "$HOOKCOPY" || ! grep -q "NEUTERED-BUILDVERB" "$HOOKCOPY"; then
    echo "SKIP:selfcheck-anchor-pending (hook lacks single-line BLOCK_VERBS/BUILD_VERB anchors)"
elif [ "$(feed_hook "$HOOKCOPY" "ls")" -ne 0 ]; then
    echo "SKIP:selfcheck-copy-broken (neutered copy cannot classify a benign command)"
elif [ "$(feed_hook "$HOOKCOPY" "cargo test")" -eq 2 ]; then
    echo "VACUOUS: neutered hook still fires on 'cargo test' — corpus cannot detect a dead guard"
    FAIL=1
else
    echo "PASS(self-check): neutered hook no longer fires on 'cargo test'"
fi
rm -f "$HOOKCOPY"

echo "== receipt log (dependency: hook emits receipts) =="
if [ ! -f "$RECEIPT_LOG" ]; then
    echo "SKIP:receipt-log-pending ($RECEIPT_LOG does not exist yet)"
else
    BLOCKS=$(grep -c "block" "$RECEIPT_LOG" 2>/dev/null || true)
    ALLOWS=$(grep -c "allow" "$RECEIPT_LOG" 2>/dev/null || true)
    BLOCKS=${BLOCKS:-0}
    ALLOWS=${ALLOWS:-0}
    if [ "$BLOCKS" -eq 0 ] && [ "$ALLOWS" -eq 0 ]; then
        echo "SKIP:receipt-log-format-pending (log exists but carries no block/allow lines yet)"
    elif [ "$BLOCKS" -ge "${#MUST_FIRE[@]}" ] && [ "$ALLOWS" -ge "${#MUST_PASS[@]}" ]; then
        echo "PASS(receipts): $BLOCKS block lines >= ${#MUST_FIRE[@]}, $ALLOWS allow lines >= ${#MUST_PASS[@]}"
    else
        echo "VACUOUS: receipt log under-counted (block=$BLOCKS want >= ${#MUST_FIRE[@]}, allow=$ALLOWS want >= ${#MUST_PASS[@]})"
        FAIL=1
    fi
fi

if [ "$FAIL" -ne 0 ]; then
    echo "RESULT: FAIL (vacuous cases present)"
    exit 1
fi
echo "RESULT: PASS"
exit 0
