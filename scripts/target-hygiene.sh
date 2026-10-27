#!/usr/bin/env bash
# target-hygiene.sh — target/ disk hygiene for /Users/sac/purrdf (lane L19).
#
# Budget policy (graphlaw precedent): PURRDF_TARGET_BUDGET_GB (default 20) ceiling,
# PURRDF_TARGET_PRUNE_DAYS (default 14) prune cutoff for idle lane dirs.
#
# Safety invariants:
#   - Deletions only ever target target-lane-* dirs or target/debug/incremental/*
#     subdirs — never target/ itself, never the whole tree. Every deletion goes
#     through rmtree_one() (python3 shutil.rmtree, ONE explicit directory per
#     invocation); the script contains zero shell recursive-delete commands.
#   - An idle lane dir is removed only if a non-blocking flock probe on its
#     .lease succeeds; a HELD lease is never removed regardless of mtime.
#   - ICE recovery (`ice <crate>`) deletes only
#     target/debug/incremental/<crate>-* — the pinned fix for incremental-cache
#     corruption (missing pre-lto.bc), never the whole incremental tree.
#   - fast-scope: prune --fast-scope deletes only stale /tmp/purrdf-fast-t*
#     measurement leases (>1 day = orphan, build-architecture.md §8) — never
#     /tmp/purrdf-fast-target (best-effort shared cold cache) and never
#     /tmp/purrdf-sccache (self-evicting at its own SCCACHE_CACHE_SIZE;
#     deleting it would evict the hit-rate lever, not reclaim waste).
#
# Fast status: `status --fast` reads sizes from scripts/.target-hygiene-state
# (written only by an explicit `audit` run, which does the full du walk) so the
# common check is sub-50ms. `status` (no flag) and `audit` always re-du, under
# a wall-clock ceiling (PURRDF_HYGIENE_CEILING_MS, default 5000) that degrades
# loudly to typed BOUNDED-SKIP lines instead of an unbounded walk. `drift`
# keeps a per-crate incremental size baseline in scripts/.target-hygiene-drift
# and warns when a crate's growth trends toward the leaf-probe 5s breach
# (PURRDF_DRIFT_WARN_KB, default 262144); prune drops baselines it evicts.
#
# Usage: see usage() at the bottom (status|audit|prune|ice|drift + knobs).
set -euo pipefail

# Timed-ceiling policy: the unbounded-cost phases here are full-tree du walks
# over a 19G target/. PURRDF_HYGIENE_CEILING_MS (default 5000) caps cumulative
# wall-clock from invocation start; a breach degrades LOUDLY — one typed
# BOUNDED-SKIP line per skipped phase on stderr — never a silent pass.
CEILING_MS="${PURRDF_HYGIENE_CEILING_MS:-5000}"
T0_NS="$(date +%s%N)"

elapsed_ms() { echo $(( ( $(date +%s%N) - T0_NS ) / 1000000 )); }
budget_ok()  { (( $(elapsed_ms) < CEILING_MS )); }

# bounded_skip <what> <detail> — typed, loud degradation notice (stderr).
bounded_skip() {
  printf 'target-hygiene: BOUNDED-SKIP %s: %s (elapsed %sms, ceiling %sms; knob: PURRDF_HYGIENE_CEILING_MS)\n' \
    "$1" "$2" "$(elapsed_ms)" "$CEILING_MS" >&2
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${REPO_ROOT}/target"
INCREMENTAL_DIR="${TARGET_DIR}/debug/incremental"
STATE_FILE="${REPO_ROOT}/scripts/.target-hygiene-state"
DRIFT_FILE="${REPO_ROOT}/scripts/.target-hygiene-drift"
BUDGET_GB="${PURRDF_TARGET_BUDGET_GB:-20}"
PRUNE_DAYS="${PURRDF_TARGET_PRUNE_DAYS:-14}"
DRIFT_WARN_KB="${PURRDF_DRIFT_WARN_KB:-262144}"

# Fast-lane target family (build-architecture.md §8): the shared cold target,
# the per-run measurement lease dirs, and the sccache cache live in /tmp,
# outside the repo-root scope above.
FAST_TARGET_DIR="/tmp/purrdf-fast-target"
FAST_LEASE_GLOB="/tmp/purrdf-fast-t[1-4]"
SCCACHE_DIR="/tmp/purrdf-sccache"
FAST_BUDGET_GB="${PURRDF_FAST_BUDGET_GB:-30}"
FAST_LEASE_MAX_AGE_DAYS=1

die() { printf 'target-hygiene: ERROR: %s\n' "$1" >&2; exit 2; }

# rmtree_one <dir> — delete exactly one directory, fail-closed if it vanished.
# Lane C5 hardening: python3 shutil.rmtree instead of a shell recursive delete.
# so every deletion is a single explicit positional argument (no glob expansion
# ever reaches a recursive-delete flag).
rmtree_one() {
  local dir="$1"
  [[ -d "$dir" ]] || { printf 'target-hygiene: ERROR: refusing to delete, not a directory: %s\n' "$dir" >&2; exit 1; }
  python3 -c "import shutil,sys; shutil.rmtree(sys.argv[1])" "$dir"
}

# Non-blocking flock probe: exit 0 iff the lease on <file> is FREE (idle).
lease_free() {
  local file="$1"
  [[ -e "$file" ]] || return 0 # no lease file => no holder recorded => idle
  ( flock -n 9 ) 9<"$file" 2>/dev/null
}

dir_size_human() {
  du -sh "$1" 2>/dev/null | cut -f1
}

dir_age_days() {
  # mtime age in whole days (0 for today).
  local mtime
  mtime=$(stat -f %m "$1" 2>/dev/null || stat -c %Y "$1" 2>/dev/null || echo 0)
  echo $(( ( $(date +%s) - mtime ) / 86400 ))
}

# Cached size lookup for --fast: format is "<path>\t<human_size>" lines.
cached_size() {
  local key="$1"
  awk -F'\t' -v k="$key" '$1 == k { print $2; found=1; exit } END { if (!found) exit 3 }' "$STATE_FILE" 2>/dev/null
}

write_state() {
  {
    printf 'state_version\t1\n'
    printf 'written_at\t%s\n' "$(date +%s)"
    printf '%s\t%s\n' "${TARGET_DIR}" "$(dir_size_human "${TARGET_DIR}")"
    if [[ -d "${INCREMENTAL_DIR}" ]]; then
      printf '%s\t%s\n' "${INCREMENTAL_DIR}" "$(dir_size_human "${INCREMENTAL_DIR}")"
    fi
    local lane
    for lane in "${TARGET_DIR}"/target-lane-*; do
      [[ -d "$lane" ]] || continue
      printf '%s\t%s\n' "$lane" "$(dir_size_human "$lane")"
    done
  } > "${STATE_FILE}.tmp"
  mv "${STATE_FILE}.tmp" "$STATE_FILE"
}

cmd_status() {
  if [[ "${1:-}" == "--fast" ]]; then
    [[ -r "$STATE_FILE" ]] || die "no state file at ${STATE_FILE}; run 'audit' first to populate cached sizes"
    local total
    total=$(cached_size "${TARGET_DIR}") || die "state file stale/incomplete; run 'audit' to refresh"
    printf 'target/ total (cached %ss ago): %s\n' "$(( $(date +%s) - $(awk -F'\t' '$1=="written_at"{print $2}' "$STATE_FILE") ))" "$total"
    if [[ -d "${INCREMENTAL_DIR}" ]]; then
      printf 'target/debug/incremental: %s\n' "$(cached_size "${INCREMENTAL_DIR}" || echo '?')"
    else
      printf 'target/debug/incremental: absent\n'
    fi
    local lane
    for lane in "${TARGET_DIR}"/target-lane-*; do
      [[ -d "$lane" ]] || continue
      if lease_free "${lane}/.lease"; then
        printf '%s: %s (idle, %sd old)\n' "$(basename "$lane")" "$(cached_size "$lane" || echo '?')" "$(dir_age_days "$lane")"
      else
        printf '%s: %s (HELD, %sd old)\n' "$(basename "$lane")" "$(cached_size "$lane" || echo '?')" "$(dir_age_days "$lane")"
      fi
    done
    return 0
  fi
  if [[ "${1:-}" == "--fast-scope" ]]; then
    # build-architecture.md §8: the fast-lane /tmp family, reported against its
    # own budget. Advisory-only — sccache is never deleted here.
    local p
    local fast_entries=()
    [[ -e "${FAST_TARGET_DIR}" ]] && fast_entries+=("${FAST_TARGET_DIR}")
    for p in ${FAST_LEASE_GLOB}; do
      [[ -d "$p" ]] && fast_entries+=("$p")
    done
    [[ -d "${SCCACHE_DIR}" ]] && fast_entries+=("${SCCACHE_DIR}")
    if (( ${#fast_entries[@]} == 0 )); then
      printf 'fast-scope: no /tmp fast-lane paths present (nothing to report)\n'
      return 0
    fi
    for p in "${fast_entries[@]}"; do
      if [[ "$p" == "${SCCACHE_DIR}" ]]; then
        local sccache_kb
        sccache_kb=$(du -sk "$p" 2>/dev/null | cut -f1)
        printf 'sccache (advisory): %s (%sGB, budget %sGB via PURRDF_FAST_BUDGET_GB) — self-evicting at SCCACHE_CACHE_SIZE, never pruned here\n' \
          "$(dir_size_human "$p")" "$(( sccache_kb / 1048576 ))" "$FAST_BUDGET_GB"
        if (( sccache_kb > FAST_BUDGET_GB * 1048576 )); then
          printf 'target-hygiene: WARNING: sccache %sGB exceeds fast budget %sGB (advisory only — self-evicting, not pruned here)\n' \
            "$(( sccache_kb / 1048576 ))" "$FAST_BUDGET_GB" >&2
        fi
      else
        printf '%s: %s (%sd old)\n' "$p" "$(dir_size_human "$p")" "$(dir_age_days "$p")"
      fi
    done
    return 0
  fi
  if ! budget_ok; then
    bounded_skip 'status' 'full-tree du walk skipped; falling through to cached fast-mode output'
    cmd_status --fast
    return 0
  fi
  printf 'target/ total: %s\n' "$(dir_size_human "${TARGET_DIR}")"
  if budget_ok; then
    if [[ -d "${INCREMENTAL_DIR}" ]]; then
      printf 'target/debug/incremental: %s\n' "$(dir_size_human "${INCREMENTAL_DIR}")"
    else
      printf 'target/debug/incremental: absent\n'
    fi
  else
    bounded_skip 'status incremental' 'du walk skipped'
  fi
  local lane found=0 size
  for lane in "${TARGET_DIR}"/target-lane-*; do
    [[ -d "$lane" ]] || continue
    found=1
    if budget_ok; then
      size=$(dir_size_human "$lane")
    else
      bounded_skip "status lane $(basename "$lane")" 'du walk skipped; using cached size'
      size=$(cached_size "$lane" || echo '?')
    fi
    if lease_free "${lane}/.lease"; then
      printf '%s: %s (idle, %sd old)\n' "$(basename "$lane")" "$size" "$(dir_age_days "$lane")"
    else
      printf '%s: %s (HELD, %sd old)\n' "$(basename "$lane")" "$size" "$(dir_age_days "$lane")"
    fi
  done
  [[ "$found" -eq 1 ]] || printf 'no target-lane-* dirs\n'
}

cmd_audit() {
  write_state
  if budget_ok; then
    printf 'audit complete: state written to %s (%sms full du walk, ceiling %sms)\n' \
      "$STATE_FILE" "$(elapsed_ms)" "$CEILING_MS"
  else
    bounded_skip 'audit' "du walk exceeded ceiling — state WAS still written to ${STATE_FILE}; sizes may already be stale"
  fi
  cmd_status --fast
}

# cmd_prune_fast_scope <dry_run> — build-architecture.md §8 rule 2: this scope
# owns only the /tmp/purrdf-fast-t* measurement leases; an entry older than
# FAST_LEASE_MAX_AGE_DAYS (1 day) is an orphan by definition. Every candidate
# is listed WOULD-PRUNE first (the dry-run listing always prints, even on a
# live run), then removed via fail-closed rmtree_one — one explicit directory
# per deletion, never a glob, never /tmp/purrdf-fast-target, never sccache.
cmd_prune_fast_scope() {
  local dry_run="$1"
  local now
  now=$(date +%s)
  printf 'fast-scope prune policy: stale /tmp/purrdf-fast-t* entries older than %sd (sccache and the shared fast target are never touched)\n' \
    "$FAST_LEASE_MAX_AGE_DAYS"
  local entry path age
  for entry in ${FAST_LEASE_GLOB}; do
    [[ -d "$entry" ]] || continue
    path="$entry"
    age=$(( ( now - $(stat -f %m "$path" 2>/dev/null || echo "$now") ) / 86400 ))
    if (( age < FAST_LEASE_MAX_AGE_DAYS )); then
      printf 'KEEP  %s (age %sd < %sd)\n' "$path" "$age" "$FAST_LEASE_MAX_AGE_DAYS"
      continue
    fi
    printf 'WOULD-PRUNE  %s (age %sd >= %sd, %s)\n' "$path" "$age" "$FAST_LEASE_MAX_AGE_DAYS" "$(dir_size_human "$path")"
    (( dry_run )) && continue
    printf 'PRUNE %s\n' "$path"
    rmtree_one "$path"
  done
}

cmd_prune() {
  local dry_run=0
  [[ "${1:-}" == "--dry-run" || "${2:-}" == "--dry-run" ]] && dry_run=1
  if [[ "${1:-}" == "--fast-scope" || "${2:-}" == "--fast-scope" ]]; then
    cmd_prune_fast_scope "$dry_run"
    return $?
  fi
  local cutoff=$(( PRUNE_DAYS * 86400 ))
  local now
  now=$(date +%s)

  if (( dry_run )); then
    printf 'DRY-RUN: no deletions will be performed.\n'
  fi
  printf 'policy: budget=%sGB prune_cutoff=%sd\n' "$BUDGET_GB" "$PRUNE_DAYS"

  # Phase 1: stale idle lane dirs older than the cutoff (HELD never touched).
  local lane
  for lane in "${TARGET_DIR}"/target-lane-*; do
    [[ -d "$lane" ]] || continue
    local age
    age=$(( now - $(stat -f %m "$lane" 2>/dev/null || echo "$now") ))
    if (( age < cutoff )); then
      printf 'KEEP  %s (age %sd < %sd)\n' "$(basename "$lane")" "$(( age / 86400 ))" "$PRUNE_DAYS"
      continue
    fi
    if ! lease_free "${lane}/.lease"; then
      printf 'KEEP  %s (stale by age but lease HELD — never removed)\n' "$(basename "$lane")"
      continue
    fi
    if (( dry_run )); then
      printf 'WOULD PRUNE  %s (idle, age %sd >= %sd, lease free, %s)\n' \
        "$(basename "$lane")" "$(( age / 86400 ))" "$PRUNE_DAYS" "$(dir_size_human "$lane")"
    else
      printf 'PRUNE %s (idle, age %sd, lease free)\n' "$(basename "$lane")" "$(( age / 86400 ))"
      rmtree_one "$lane"
    fi
  done

  # Phase 2: budget enforcement — evict target/debug/incremental/* oldest-first.
  if [[ ! -d "${INCREMENTAL_DIR}" ]]; then
    printf 'incremental absent; budget phase skipped\n'
    return 0
  fi
  # EXACT decrement accounting, in KB throughout: a GB-rounded total minus
  # GB-rounded victims drifts both directions (du -sg floors the total,
  # ceil(victim) over-subtracts — the loop can under-evict and leave the tree
  # over budget). Budget converts to KB once; every subtraction is exact.
  local used_kb budget_kb=$(( BUDGET_GB * 1048576 ))
  if ! budget_ok; then
    bounded_skip 'prune phase2' 'initial target/ du walk skipped (ceiling); budget enforcement not attempted this run'
    return 0
  fi
  used_kb=$(du -sk "${TARGET_DIR}" 2>/dev/null | cut -f1)
  if (( used_kb <= budget_kb )); then
    printf 'target/ at %sGB within budget %sGB; incremental eviction not needed\n' "$(( used_kb / 1048576 ))" "$BUDGET_GB"
    return 0
  fi
  printf 'target/ at %sGB exceeds budget %sGB; evicting incremental oldest-first\n' "$(( used_kb / 1048576 ))" "$BUDGET_GB"
  local entry
  # shellcheck disable=SC2012  # deterministic oldest-first ordering is the point
  for entry in $(ls -1t "${INCREMENTAL_DIR}" | tail -r 2>/dev/null || ls -1tr "${INCREMENTAL_DIR}"); do
    (( used_kb <= budget_kb )) && break
    local victim="${INCREMENTAL_DIR}/${entry}"
    [[ -d "$victim" ]] || continue
    if ! budget_ok; then
      bounded_skip "prune eviction incremental/${entry}" 'ceiling reached; stopping the eviction loop before sizing the next victim'
      break
    fi
    local victim_kb
    victim_kb=$(du -sk "$victim" 2>/dev/null | cut -f1)
    if (( dry_run )); then
      printf 'WOULD EVICT  incremental/%s (%sKB)\n' "$entry" "$victim_kb"
    else
      printf 'EVICT incremental/%s (%sKB)\n' "$entry" "$victim_kb"
      rmtree_one "$victim"
      drift_forget "$entry"
    fi
    # Exact decrement: subtract the victim's measured KB (no rounding) from
    # the KB total. Phase 2 cost stays 2 full walks + N victim-only walks,
    # and the running total tracks ground truth to within the victims' own
    # du noise; the final walk re-syncs anyway.
    used_kb=$(( used_kb - victim_kb ))
    (( used_kb < 0 )) && used_kb=0
  done
  if budget_ok; then
    used_kb=$(du -sk "${TARGET_DIR}" 2>/dev/null | cut -f1)
  else
    bounded_skip 'prune phase2 final du' 'ceiling reached; post-prune total is the decremented estimate, not a fresh walk'
  fi
  printf 'post-prune target/: %sGB (budget %sGB)\n' "$(( used_kb / 1048576 ))" "$BUDGET_GB"
  if (( used_kb > budget_kb )); then
    printf 'target-hygiene: WARNING: budget still exceeded after evicting all incremental entries\n' >&2
    return 1
  fi
}

cmd_ice() {
  local crate="${1:-}"
  [[ -z "$crate" ]] && die 'ice <crate>: missing crate name'
  local matches=("${INCREMENTAL_DIR}"/"${crate}"-*)
  local found=0 e
  for e in "${matches[@]}"; do
    [[ -d "$e" ]] || continue
    found=1
    if [[ "${DRY_RUN_ICE:-0}" == "1" ]]; then
      printf 'WOULD DELETE %s\n' "$e"
    else
      if ! budget_ok; then
        printf 'target-hygiene: ERROR: BOUNDED-SKIP ice %s: ceiling %sms reached before deleting %s; raise PURRDF_HYGIENE_CEILING_MS or set DRY_RUN_ICE=1 to preview\n' \
          "$crate" "$CEILING_MS" "$e" >&2
        exit 3
      fi
      printf 'ICE-RECOVERY: deleting corrupted incremental cache %s\n' "$e"
      rmtree_one "$e"
    fi
  done
  if (( ! found )); then
    printf 'target-hygiene: ERROR: no target/debug/incremental/%s-* dir found; nothing to recover\n' "$crate" >&2
    exit 1
  fi
  printf 'ICE-RECOVERY: incremental cache for crate %s cleared; rebuild the crate to recover\n' "$crate"
}

# drift_forget <crate-entry> — drop one crate's row from the drift baseline so
# a prune-evicted crate's next drift census reads as new (no stale trend
# against a pre-eviction size).
drift_forget() {
  local crate="$1"
  [[ -f "$DRIFT_FILE" ]] || return 0
  local dtmp="${DRIFT_FILE}.tmp"
  awk -F'\t' -v k="$crate" '$1 != k { print }' "$DRIFT_FILE" > "$dtmp"
  mv "$dtmp" "$DRIFT_FILE"
}

# drift — per-crate incremental-cache size census with growth trend against
# the previous census. Growth above PURRDF_DRIFT_WARN_KB (default 262144 KB =
# 256MB) is the early SLA tripwire: a crate trending toward the leaf-probe 5s
# breach before it gets there. Ceiling breach mid-census → per-crate
# BOUNDED-SKIP, partial output kept, skipped crates' old baselines carried.
cmd_drift() {
  [[ $# -eq 0 ]] || die 'drift takes no arguments'
  if [[ ! -d "${INCREMENTAL_DIR}" ]]; then
    printf 'drift: incremental absent; nothing to census\n'
    return 0
  fi
  local tmp="${DRIFT_FILE}.tmp"
  : > "$tmp"
  printf 'state_version\t1\n' >> "$tmp"
  printf 'written_at\t%s\n' "$(date +%s)" >> "$tmp"
  local path entry kb prev growth skipped=0 sign
  for path in "${INCREMENTAL_DIR}"/*; do
    [[ -d "$path" ]] || continue
    entry="$(basename "$path")"
    if budget_ok; then
      kb=$(du -sk "$path" 2>/dev/null | cut -f1)
      printf '%s\t%s\n' "$entry" "$kb" >> "$tmp"
      prev=$(awk -F'\t' -v k="$entry" '$1 == k { print $2; found=1; exit } END { if (!found) exit 3 }' "$DRIFT_FILE" 2>/dev/null) || true
      if [[ -n "$prev" ]]; then
        growth=$(( kb - prev ))
        if (( growth < 0 )); then sign=''; else sign='+'; fi
        printf 'drift: %s: %sKB (%s%sKB since last drift)\n' "$entry" "$kb" "$sign" "$growth"
        if (( growth > DRIFT_WARN_KB )); then
          printf 'target-hygiene: WARNING: %s trending toward leaf-probe %sms breach (grew %sKB > PURRDF_DRIFT_WARN_KB=%sKB)\n' \
            "$entry" "$CEILING_MS" "$growth" "$DRIFT_WARN_KB" >&2
        fi
      else
        printf 'drift: %s: %sKB (new since last drift)\n' "$entry" "$kb"
      fi
    else
      skipped=$(( skipped + 1 ))
      bounded_skip "drift ${entry}" 'per-crate du skipped (ceiling); keeping partial output'
    fi
  done
  # Preserve baselines for crates this run skipped so their trend history
  # survives a partial census.
  if (( skipped > 0 )) && [[ -f "$DRIFT_FILE" ]]; then
    local carried
    carried=$(awk -F'\t' 'NR == FNR { seen[$1] = 1; next } $1 != "state_version" && $1 != "written_at" && !($1 in seen) { print }' "$tmp" "$DRIFT_FILE") || true
    if [[ -n "$carried" ]]; then
      printf '%s\n' "$carried" >> "$tmp"
    fi
  fi
  mv "$tmp" "$DRIFT_FILE"
}

usage() {
  printf '%s\n' \
    'usage: target-hygiene.sh status [--fast] [--fast-scope] | audit | prune [--dry-run] [--fast-scope] | ice <crate> | drift' \
    '' \
    'subcommands:' \
    '  status [--fast]   target/ + incremental + lane sizes (fast reads cached state)' \
    '  status --fast-scope  du the /tmp fast-lane family (/tmp/purrdf-fast-target,' \
    '                       /tmp/purrdf-fast-t*, /tmp/purrdf-sccache vs PURRDF_FAST_BUDGET_GB,' \
    '                       default 30; sccache advisory-only, never pruned)' \
    '  audit             full du walk, writes scripts/.target-hygiene-state' \
    '  prune [--dry-run] stale-idle-lane prune + budget enforcement (evicts incremental oldest-first)' \
    '  prune --fast-scope [--dry-run]  delete /tmp/purrdf-fast-t* entries older than 1 day' \
    '                       (WOULD-PRUNE listing always prints first; never touches sccache' \
    '                       or /tmp/purrdf-fast-target)' \
    '  ice <crate>       delete corrupted target/debug/incremental/<crate>-* (DRY_RUN_ICE=1 previews)' \
    '  drift             per-crate incremental size census + growth vs scripts/.target-hygiene-drift' \
    '' \
    'knobs:' \
    '  PURRDF_HYGIENE_CEILING_MS  wall-clock ceiling for du phases (default 5000; breach = BOUNDED-SKIP)' \
    '  PURRDF_TARGET_BUDGET_GB    prune eviction budget (default 20)' \
    '  PURRDF_TARGET_PRUNE_DAYS   idle-lane prune cutoff (default 14)' \
    '  PURRDF_DRIFT_WARN_KB       drift growth warning threshold (default 262144)' \
    '  PURRDF_FAST_BUDGET_GB      fast-scope sccache advisory budget (default 30)' \
    '  DRY_RUN_ICE=1              ice: list would-delete targets, no deletion' >&2
  exit 2
}

case "${1:-}" in
  status) shift; cmd_status "$@" ;;
  audit)  shift; [[ $# -eq 0 ]] || die 'audit takes no arguments'; cmd_audit ;;
  prune)  shift; cmd_prune "$@" ;;
  ice)    shift; cmd_ice "$@" ;;
  drift)  shift; cmd_drift "$@" ;;
  *) usage ;;
esac
