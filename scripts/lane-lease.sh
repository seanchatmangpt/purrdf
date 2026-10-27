#!/usr/bin/env bash
# lane-lease.sh — per-lane CARGO_TARGET_DIR isolation with an exclusive lease
# (lane C6, /Users/sac/purrdf).
#
# Lease roots (C6 change):
#   primary : /tmp/purrdf-targets/lane-<name>   — all NEW leases live here;
#             throwaway by construction (host /tmp), cleaned with the host.
#   legacy  : <repo>/target-lane-<name>         — backward compat only. `list`
#             still reports legacy dirs so held/in-flight lanes stay visible.
#             `exec` never creates a legacy dir.
#   The shared warm cache <repo>/target/ is untouched: it is the reusable
#   workspace target/, lanes are disposable build roots layered over it.
#
# Usage:
#   lane-lease.sh exec [--wait <ms>] <name> -- CMD...   run CMD in /tmp/purrdf-targets/lane-<name>
#   lane-lease.sh warm [--wait <ms>] <name> -- CMD...   prewarm: run CMD with output
#                          discarded, mark <lane>/.warm, release the lease on return
#   lane-lease.sh clean [<name>]                        delete one lane (refuses HELD,
#                          exit 3) or all IDLE primary-root lanes (legacy never touched)
#   lane-lease.sh list                                  show each lane dir + HELD/idle + warmth
#
# exec: mkdir -p the lane dir, takes a non-blocking exclusive flock on
# lane/.lease held for the lifetime of CMD (fd 9, inherited), exports
# CARGO_TARGET_DIR=<lane> and CARGO_BUILD_JOBS (default 2), then execs CMD.
#   Default: fail-fast — if the lease is HELD, typed exit 3 immediately
#   (flock -n, no queuing).
#   --wait <ms>: bounded contention wait — poll flock -n every 50ms up to
#   <ms> total (default 500 when --wait given bare; no unbounded queuing);
#   on expiry, typed exit 3.
#
# clean: refuses a HELD lane (fd-8 probe held through the rm to shrink the
# TOCTOU window); bare `clean` walks primary-root lanes only — legacy dirs are
# structurally unreachable by construction.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEGACY_PREFIX="${REPO_ROOT}/target-lane-"
LEASE_ROOT="/tmp/purrdf-targets"
POLL_INTERVAL_MS=50

# macOS ships no /usr/bin/flock; without it every probe would read as "busy"
# (exit 127 silenced) and every lane would falsely report HELD. Fail loudly.
command -v flock >/dev/null 2>&1 || {
  printf 'lane-lease: ERROR: flock(1) not found on PATH (macOS: brew install util-linux flock)\n' >&2
  exit 2
}

die() { printf 'lane-lease: ERROR: %s\n' "$1" >&2; exit 2; }

# exit 3 with the typed REFUSED message (used by fail-fast and wait-expiry).
refused() {
  printf 'lane-lease: REFUSED: lane %s is HELD by another process (flock busy on %s)\n' "$1" "$2" >&2
  exit 3
}

# Wall-clock milliseconds (python3 clock, date fallback — portable, no %N).
now_ms() {
  python3 -c 'import time; print(int(time.time() * 1000))' 2>/dev/null \
    || printf '%s000\n' "$(date +%s)"
}

# Warmth state for one lane dir: cold, or warm(<N>s old) from the .warm stamp.
# Always ends rc 0 so command-substitution assignment is set -e safe.
lane_warm_state() {
  local lane="$1" stamp="${1}/.warm" mtime now
  [[ -e "$stamp" ]] || { printf 'cold'; return 0; }
  mtime=$(stat -f %m "$stamp" 2>/dev/null || stat -c %Y "$stamp" 2>/dev/null || echo 0)
  now=$(date +%s)
  local age=$((now - mtime))
  ((age < 0)) && age=0
  printf 'warm(%ss old)' "$age"
  return 0
}

# Non-blocking probe: flock(1) takes the FILE argument directly (`flock -n
# <file> true`). The fd-number form (`flock -n 8 <file`) never opens fd 8 —
# `<file` is stdin — and dies "Bad file descriptor", reading every released
# lane as HELD. (The same latent bug sat in the old fd-9 probe.)
lease_free8() {
  local file="$1"
  [[ -e "$file" ]] || return 0
  flock -n "$file" true 2>/dev/null
}

usage_exec() { die 'exec [--wait <ms>] <name> -- CMD...' ; }
usage_warm() { die 'warm [--wait <ms>] <name> -- CMD...' ; }

cmd_exec() {
  local wait_ms=500 name=""
  while (($#)); do
    case "$1" in
      --wait)
        [[ "${2:-}" =~ ^[0-9]+$ ]] || usage_exec
        wait_ms="$2"; shift 2 ;;
      --wait=*)
        local v="${1#--wait=}"
        [[ "$v" =~ ^[0-9]+$ ]] || usage_exec
        wait_ms="$v"; shift ;;
      *) name="$1"; shift; break ;;
    esac
  done
  [[ -n "$name" ]] || usage_exec
  # Lane name must not escape the lease root (no '/', no '..').
  [[ "$name" =~ ^[A-Za-z0-9._-]+$ && "$name" != *..* ]] || die "invalid lane name: ${name}"
  [[ "${1:-}" == "--" ]] && shift || usage_exec
  (($#)) || die 'exec <name> -- CMD...: empty CMD'

  local lane="${LEASE_ROOT}/lane-${name}"
  mkdir -p "$lane"
  local lease="${lane}/.lease"
  # touch first so the fd open never fails on a missing file.
  : >>"$lease"

  # Hold fd 9 open for the whole contention window: probing inside a subshell
  # releases the lock before exec, leaving a TOCTOU window where another
  # contender can win and mis-type the exit as 2 instead of the contract's 3.
  exec 9<"$lease"
  # Bounded wait (default 500ms): poll flock -n every 50ms; fail-fast is the
  # degenerate case (immediate refusal on first busy probe).
  local waited=0
  until flock -n 9 2>/dev/null; do
    if ((waited >= wait_ms)); then
      refused "$name" "$lease"
    fi
    sleep "0.$(printf '%03d' "$POLL_INTERVAL_MS")"
    waited=$((waited + POLL_INTERVAL_MS))
  done
  export CARGO_TARGET_DIR="$lane"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
  printf 'lane-lease: lane=%s dir=%s lease=HELD wait_ms=%s cmd=%s\n' "$name" "$lane" "$wait_ms" "$*" >&2
  exec "$@"
}

# warm: byte-for-byte mirror of exec's arg/validation/fd-9 poll (no subshell
# fd — TOCTOU), then runs CMD with output discarded, stamps .warm, releases.
cmd_warm() {
  local wait_ms=500 name=""
  while (($#)); do
    case "$1" in
      --wait)
        [[ "${2:-}" =~ ^[0-9]+$ ]] || usage_warm
        wait_ms="$2"; shift 2 ;;
      --wait=*)
        local v="${1#--wait=}"
        [[ "$v" =~ ^[0-9]+$ ]] || usage_warm
        wait_ms="$v"; shift ;;
      *) name="$1"; shift; break ;;
    esac
  done
  [[ -n "$name" ]] || usage_warm
  [[ "$name" =~ ^[A-Za-z0-9._-]+$ && "$name" != *..* ]] || die "invalid lane name: ${name}"
  [[ "${1:-}" == "--" ]] && shift || usage_warm
  (($#)) || die 'warm <name> -- CMD...: empty CMD'

  local lane="${LEASE_ROOT}/lane-${name}"
  mkdir -p "$lane"
  local lease="${lane}/.lease"
  : >>"$lease"

  exec 9<"$lease"
  local waited=0
  until flock -n 9 2>/dev/null; do
    if ((waited >= wait_ms)); then
      printf 'lane-lease: REFUSED-WARM: lane %s warm exceeded wait (%sms; flock busy on %s)\n' "$name" "$wait_ms" "$lease" >&2
      exit 3
    fi
    sleep "0.$(printf '%03d' "$POLL_INTERVAL_MS")"
    waited=$((waited + POLL_INTERVAL_MS))
  done
  export CARGO_TARGET_DIR="$lane"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
  local t0 rc
  t0=$(now_ms)
  if "$@" >/dev/null 2>&1; then
    rc=0
  else
    rc=$?
  fi
  if [[ "$rc" -ne 0 ]]; then
    printf 'lane-lease: warm command failed rc=%s for lane %s\n' "$rc" "$name" >&2
    return "$rc"
  fi
  touch "${lane}/.warm"
  local t1
  t1=$(now_ms)
  printf 'lane-lease: warmed lane=%s in %sms\n' "$name" "$((t1 - t0))"
}

# clean [<name>]: refuse HELD (fd-8 probe held through rm), delete, or bare
# = every IDLE primary-root lane. Legacy dirs are never reachable.
cmd_clean() {
  mkdir -p "$LEASE_ROOT"
  if [[ -n "${1:-}" ]]; then
    local name="$1"
    [[ "$name" =~ ^[A-Za-z0-9._-]+$ && "$name" != *..* ]] || die "invalid lane name: ${name}"
    local lane="${LEASE_ROOT}/lane-${name}"
    [[ -d "$lane" ]] || die "no such lane: ${name}"
    local lease="${lane}/.lease"
    if [[ -e "$lease" ]]; then
      # Delete UNDER the lock (flock holds it for the rm) so a contender that
      # grabs the lock between probe and rm cannot race the deletion.
      flock -n "$lease" rm -rf "$lane" 2>/dev/null || {
        printf 'lane-lease: lane %s is HELD; refusing to delete (%s)\n' "$name" "$lease" >&2
        exit 3
      }
    else
      rm -rf "$lane"
    fi
    printf 'lane-lease: cleaned lane=%s\n' "$name"
    return 0
  fi
  # Bare clean: primary-root lanes only, HELD skipped loudly.
  local lane
  for lane in "${LEASE_ROOT}"/lane-*; do
    [[ -d "$lane" ]] || continue
    local lease="${lane}/.lease"
    if [[ -e "$lease" ]] && ! lease_free8 "$lease"; then
      printf 'lane-lease: skip HELD lane %s\n' "$(basename "$lane")" >&2
      continue
    fi
    rm -rf "$lane"
  done
  printf 'lane-lease: cleaned idle lanes in %s\n' "$LEASE_ROOT"
}

cmd_list() {
  mkdir -p "$LEASE_ROOT"
  local lane found=0
  # primary root first, then legacy target-lane-* (backward compat visibility).
  for lane in "${LEASE_ROOT}"/lane-* "${LEGACY_PREFIX}"*; do
    [[ -d "$lane" ]] || continue
    found=1
    local state
    if lease_free8 "${lane}/.lease"; then state=idle; else state=HELD; fi
    printf '%s: %s (%s, %s)\n' "$(basename "$lane")" "$(du -sh "$lane" 2>/dev/null | cut -f1)" "$state" "$(lane_warm_state "$lane")"
  done
  [[ "$found" -eq 1 ]] || printf "no lane dirs in ${LEASE_ROOT} or ${LEGACY_PREFIX}*\n"
}

case "${1:-}" in
  exec) shift; cmd_exec "$@" ;;
  warm) shift; cmd_warm "$@" ;;
  clean) shift; cmd_clean "$@" ;;
  list) cmd_list ;;
  *) die 'usage: lane-lease.sh exec [--wait <ms>] <name> -- CMD... | warm [--wait <ms>] <name> -- CMD... | clean [<name>] | list' ;;
esac
