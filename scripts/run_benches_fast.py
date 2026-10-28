#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Fast two-phase bench orchestrator (stdlib-only).

Splits `cargo bench` into compile (Mode A) and measure (Mode B):

  Mode A (compile)   : `cargo bench -p CRATE --no-run --message-format=json`
                       per crate into a shared /tmp/purrdf-fast-target
                       (offline, jobs=8, incremental off, sccache when
                       present); bench binary paths are resolved from the
                       JSON "executable" messages keyed by package, with a
                       freshness-checked deps-dir glob only as a fallback
                       (binary mtime must postdate compile start, else a
                       typed BenchError). Harness=true [[bench]] targets are
                       refused up front — the fast orchestrator only runs
                       harness=false plain executables. Standalone `measure`
                       re-resolves binaries from the prior compile log; when
                       that fails it refuses (typed BenchError) unless
                       `--allow-stale-binaries` is passed explicitly.
  Mode B (measure)   : worker pool (max 4 concurrent) runs each bench binary
                       directly, each worker with its own
                       CARGO_TARGET_DIR=/tmp/purrdf-fast-t<N> lease (created
                       on start, deleted at exit — lease law; lease claim is
                       guarded by an flock on the lease's `.lock` file and a
                       lease holding a live foreign pid is occupied;
                       owner.pid records the WORKER pid), output to
                       /tmp/bench-logs/<crate>.<bench>.fast.log, one register
                       row `CRATE BENCH=NAME rc=R SECS=S log=...` appended
                       under an exclusive flock to
                       /tmp/bench-logs/register-fast.txt via a single
                       O_APPEND write. Signal deaths are normalized in
                       register rows: a worker killed by SIGKILL (raw rc -9
                       on POSIX) is recorded as rc=137, and a CAP-expiry kill
                       as rc=124 — rc is never None. A killed (rc=137)
                       attempt records its own row, then is retried ONCE and
                       SOLO: the worker pool is drained to EMPTY before the
                       retry spawns, so no other bench is in flight; the
                       retry gets a second row marked ` retry=1`. A green
                       retry makes the bench green (reds accounting is per
                       bench, decided by the retry). Each run is capped at
                       CAP seconds (killed -> rc=124 row).

  `--resume` skips a crate only when EVERY bench name from its manifest has
  a green (rc=0) register row; without --resume no green row skips any
  re-measurement. SIGTERM/SIGINT/SIGHUP are installed once at module level
  against a mutable active-workers registry: every spawned child (compile
  child included) runs in its own process group (start_new_session) and the
  handler group-terminates, bounded-waits, then group-kills everything
  registered, removes all leases and exits non-zero; an atexit backstop
  repeats the cleanup. The only sleep is the 0.2s worker poll (bounded, on a
  real observable: Popen.poll).

Bench-flag passthrough (forwarded to each binary after `--bench`):
  --quick --exact --filter F (repeatable) and exactly one of
  --save-baseline NAME | --baseline NAME | --baseline-lenient NAME.
  Default (no flags) behavior is identical to the flagless invocation.

Usage:
  run_benches_fast.py compile <crates...>
  run_benches_fast.py measure <crates...>
  run_benches_fast.py run <crates...>       # both phases
  run_benches_fast.py run --resume <crates...>
"""
from __future__ import annotations

import argparse
import atexit
import fcntl
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import tomllib

LOGS = "/tmp/bench-logs"
REGISTER = os.path.join(LOGS, "register-fast.txt")
RUNNER_LOG = os.path.join(LOGS, "runner-fast.log")
# root for the worker lease dirs (/tmp default); injected by the court
LEASE_ROOT = "/tmp"
SHARED_TARGET = "/tmp/purrdf-fast-target"
DEPS_DIR = os.path.join(SHARED_TARGET, "release", "deps")
CRATES_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates"))
CAP = 3600
MAX_WORKERS = 4
POLL = 0.2
NOT_BINARY = (".d", ".rlib", ".rmeta", ".so", ".a")
TERMINATE_GRACE = 5.0
# glob fallback: binary must be fresher than the compile that produced it
FRESHNESS_SLACK = 2.0
BENCH_STORE = "/tmp/purrdf-bench-store"
MACHO_MAGICS = (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf")
ELF_MAGIC = b"\x7fELF"

# --- G8: module-level signal handling over a mutable active-workers registry
_ACTIVE: list[dict] = []  # entries: {"proc": Popen | None}
_HANDLERS_INSTALLED = False


class BenchError(RuntimeError):
    """Typed orchestrator failure (missing manifest, compile red, no binary)."""


def _killed(rc: int | None) -> bool:
    """True iff rc classifies as a SIGKILL death.

    subprocess returncode is -signum on POSIX (a SIGKILL'd worker yields
    -9), while a shell that relays the signal may exit 137 — both mean
    SIGKILL and both classify here (G1).
    """
    return rc == 137 or rc == -9


def _normalize_rc(raw: int | None, cap_killed: bool) -> int:
    """Register-row rc contract: never None, never a raw negative signal.

    raw None after kill() means the bounded wait expired: a CAP kill
    records 124, a forced (signal-handler / straggler) kill records 137.
    Signal deaths otherwise normalize -9 -> 137 (G1/G3).
    """
    if raw is None:
        return 124 if cap_killed else 137
    if cap_killed:
        return 124
    if _killed(raw):
        return 137
    return raw


def _signal_group(proc: subprocess.Popen, sig: int) -> None:
    """Deliver sig to the child's whole process group (G7), falling back to
    the child alone if the group is already gone."""
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except (ProcessLookupError, PermissionError, OSError):
        try:
            proc.send_signal(sig)
        except OSError:
            pass


def _cleanup_active() -> None:
    """Group-terminate, bounded-wait, group-kill every registered child."""
    live = []
    for entry in list(_ACTIVE):
        proc: subprocess.Popen | None = entry.get("proc")
        if proc is None or proc.poll() is not None:
            continue
        live.append(proc)
        try:
            _signal_group(proc, signal.SIGTERM)
        except OSError:
            pass
    deadline = time.time() + TERMINATE_GRACE
    for proc in live:
        remaining = max(0.0, deadline - time.time())
        try:
            proc.wait(timeout=remaining)
        except subprocess.TimeoutExpired:
            try:
                _signal_group(proc, signal.SIGKILL)
                proc.wait(timeout=TERMINATE_GRACE)
            except (OSError, subprocess.TimeoutExpired):
                pass
    _ACTIVE.clear()


def _on_signal(signum, _frame):  # noqa: ANN001 — signal handler signature
    log(f"signal {signum}: terminating children and releasing leases")
    _cleanup_active()
    sys.exit(128 + signum)


def _install_signal_handlers() -> None:
    """Installed exactly once for the process (G8)."""
    global _HANDLERS_INSTALLED
    if _HANDLERS_INSTALLED:
        return
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, _on_signal)
    atexit.register(_cleanup_active)
    _HANDLERS_INSTALLED = True


def log(msg: str) -> None:
    line = f"[bench-fast] {msg}"
    print(line, flush=True)
    with open(RUNNER_LOG, "a") as fh:
        fh.write(line + "\n")


def register_row(crate: str, bench: str, rc: int, secs: int, path: str,
                 retried: bool = False) -> None:
    """Single atomic O_APPEND write under an exclusive register flock.

    rc must be a concrete int (callers normalize through _normalize_rc);
    signal deaths are recorded as 137, CAP kills as 124 — never None.
    """
    assert isinstance(rc, int), f"register rc must be int, got {rc!r}"
    suffix = " retry=1" if retried else ""
    line = f"{crate} BENCH={bench} rc={rc} SECS={secs} log={path}{suffix}\n"
    os.makedirs(LOGS, exist_ok=True)
    fd = os.open(REGISTER, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
    try:
        with os.fdopen(fd, "a") as fh:
            fd = -1
            fcntl.flock(fh.fileno(), fcntl.LOCK_EX)
            fh.write(line)
            fh.flush()
            fcntl.flock(fh.fileno(), fcntl.LOCK_UN)
    finally:
        if fd >= 0:
            os.close(fd)


def _locked_register_lines() -> list[str]:
    if not os.path.exists(REGISTER):
        return []
    with open(REGISTER) as fh:
        fcntl.flock(fh.fileno(), fcntl.LOCK_SH)
        try:
            return fh.readlines()
        finally:
            fcntl.flock(fh.fileno(), fcntl.LOCK_UN)


def crate_has_green_row(crate: str, names: list[tuple[str, bool]]) -> bool:
    """--resume gate: EVERY bench name of the crate must have a green row."""
    lines = _locked_register_lines()
    prefix = f"{crate} BENCH="
    for name, _harness in names:
        if not any(
            line.startswith(f"{prefix}{name} ") and " rc=0 " in line
            for line in lines
        ):
            return False
    return True


def crate_manifest(crate: str) -> str:
    # directories are unprefixed: package purrdf-<name> lives at crates/<name>
    # (workspace-root members like crates/purrdf keep their literal dir).
    candidates = [crate]
    if crate.startswith("purrdf-"):
        candidates.insert(0, crate[len("purrdf-"):])
    for stem in candidates:
        candidate = os.path.join(CRATES_ROOT, stem, "Cargo.toml")
        if os.path.exists(candidate):
            return candidate
    # last resort: scan manifests for a matching package name (rdf-core etc.)
    for entry in sorted(os.listdir(CRATES_ROOT)):
        candidate = os.path.join(CRATES_ROOT, entry, "Cargo.toml")
        if not os.path.exists(candidate):
            continue
        with open(candidate, "rb") as fh:
            data = tomllib.load(fh)
        if data.get("package", {}).get("name") == crate:
            return candidate
    raise BenchError(f"crate manifest not found for {crate!r} under {CRATES_ROOT}")


def build_env(target_dir: str) -> dict[str, str]:
    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = target_dir
    # honor a pre-set override (sccache-fast.sh FAST_JOBS); default 8
    env.setdefault("CARGO_BUILD_JOBS", "8")
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_NET_OFFLINE"] = "true"
    sccache_bin = os.environ.get("SCCACHE_BIN") or shutil.which("sccache")
    if sccache_bin:
        env["RUSTC_WRAPPER"] = sccache_bin
    else:
        if env.pop("RUSTC_WRAPPER", None) is not None:
            print(
                "run-benches-fast[WARN]: sccache not found "
                "(SCCACHE_BIN unset, PATH lookup failed); dropping RUSTC_WRAPPER",
                file=sys.stderr,
            )
    return env


def bench_names(manifest: str) -> list[tuple[str, bool]]:
    """(name, harness) per [[bench]] target from a crate's Cargo.toml.

    A harness=true target raises a typed BenchError: the fast orchestrator
    never runs a bench bare under cargo's harness.
    """
    with open(manifest, "rb") as fh:
        data = tomllib.load(fh)
    targets = [(b["name"], bool(b.get("harness", True)))
               for b in data.get("bench", []) if b.get("name")]
    if not targets:
        raise BenchError(f"no [[bench]] name fields in {manifest}")
    for name, harness in targets:
        if harness:
            raise BenchError(
                f"harness=true unsupported by fast orchestrator: bench "
                f"{name!r} in {manifest}"
            )
    return targets


def _sniffs_executable(path: str) -> bool:
    """Executability by magic bytes (Mach-O / ELF), not the x-bit."""
    try:
        with open(path, "rb") as fh:
            head = fh.read(4)
    except OSError:
        return False
    return head.startswith(MACHO_MAGICS) or head.startswith(ELF_MAGIC)


def _find_built_binary(name: str, crate: str, compile_start: float) -> str:
    """Deps binaries carry `<name>-<16 hex>` suffixes; pick the newest fresh
    match, sniffed executable by magic bytes."""
    if not os.path.isdir(DEPS_DIR):
        raise BenchError(f"no compile output at {DEPS_DIR}; run the compile phase first")
    matches = []
    for f in os.listdir(DEPS_DIR):
        if f == name:
            pass
        elif (f.startswith(name + "-") and len(f) == len(name) + 1 + 16
              and all(c in "0123456789abcdef" for c in f[len(name) + 1:])):
            pass
        else:
            continue
        if f.endswith(NOT_BINARY):
            continue
        path = os.path.join(DEPS_DIR, f)
        if os.path.isfile(path) and _sniffs_executable(path):
            matches.append(path)
    if not matches:
        raise BenchError(f"no built binary for bench {name!r} of {crate}")
    return sorted(matches, key=lambda p: os.path.getmtime(p))[-1]


def _preflight_binary(crate: str, bench: str, binary: str) -> None:
    """`<binary> --list` must exit 0 (touches no store) before the real run.

    The probe env pins PURRDF_BENCH_HOME to BENCH_STORE (finding 19: without
    it a store touch falls back to the baked /tmp/purrdf-fast-target store),
    and the probe store dir is removed afterwards (finding 20: probe dirs
    are leases too — never left behind)."""
    probe_dir = lease_env_dir(bench)
    env = {**build_env(probe_dir), "PURRDF_BENCH_HOME": BENCH_STORE}
    try:
        probe = subprocess.run(
            [binary, "--list"], stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, env=env,
            timeout=60, start_new_session=True,
        )
    finally:
        shutil.rmtree(probe_dir, ignore_errors=True)
    if probe.returncode != 0:
        raise BenchError(
            f"pre-flight --list failed for bench {bench!r} of {crate}: "
            f"rc={probe.returncode} binary={binary}"
        )


def lease_env_dir(bench: str) -> str:
    """Store home for pre-flight probes; the worker lease is set at spawn."""
    return os.path.join(LEASE_ROOT, f"purrdf-fast-probe-{bench}")


def _bench_cmd(binary: str, bench_args: list[str] | None) -> list[str]:
    """Worker argv: `--bench` first, then operator passthrough flags (G12)."""
    return [binary, "--bench", *(bench_args or [])]


def classify_rc101(crate: str, bench: str, log_path: str) -> str:
    """rc=101 covers both genuine failure and refused command line; the log
    footer distinguishes them: `N benchmarks measured; M failed` (stdout,
    real run) vs `error:` (stderr, parse refusal)."""
    try:
        with open(log_path, "rb") as fh:
            data = fh.read()
    except OSError:
        return "unknown"
    text = data.decode("utf-8", "replace")
    if "benchmarks measured;" in text:
        return "failed"
    if "error:" in text:
        return "refused"
    return "unknown"


def _executables_from_json(json_log: str, crate: str) -> dict[str, str]:
    """name -> executable path from `cargo bench --no-run` JSON messages."""
    found: dict[str, str] = {}
    with open(json_log) as fh:
        for line in fh:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            reason = msg.get("reason")
            if reason != "compiler-artifact":
                continue
            exe = msg.get("executable")
            if not exe or msg.get("package_id") and crate not in str(msg.get("package_id")):
                continue
            base = os.path.basename(exe)
            stem = base.split("-")[0]
            if base.endswith(NOT_BINARY):
                continue
            found.setdefault(stem, exe)
    return found


def compile_phase(crate: str) -> list[tuple[str, str]]:
    """Mode A: cargo bench --no-run --message-format=json.

    Returns (bench_name, binary_path) tuples keyed by canonical manifest
    names — never names derived from binary filenames. Binary paths come
    from the cargo JSON "executable" fields; the deps glob is a fallback
    only and requires freshness (mtime postdating compile start). The cargo
    child runs in its own process group and is registered with the
    module-level signal registry so a signal group-kills it (G7/G8).
    """
    _install_signal_handlers()
    manifest = crate_manifest(crate)
    crate_dir = os.path.dirname(manifest)
    targets = bench_names(manifest)
    names = [n for n, _h in targets]
    log(f"compile {crate} starting ({len(names)} bench targets: {', '.join(names)})")
    log_path = os.path.join(LOGS, f"{crate}.compile.fast.log")
    start = time.time()
    holder: dict = {"proc": None}
    _ACTIVE.append(holder)
    try:
        with open(log_path, "wb") as fh:
            proc = subprocess.Popen(
                ["cargo", "bench", "-p", crate, "--no-run",
                 "--message-format=json"],
                cwd=crate_dir, stdout=fh, stderr=subprocess.STDOUT,
                env=build_env(SHARED_TARGET), start_new_session=True,
            )
            holder["proc"] = proc  # G9: registered before it can be missed
            try:
                proc.communicate(timeout=CAP)
                rc: int | None = proc.returncode
            except subprocess.TimeoutExpired:
                try:
                    _signal_group(proc, signal.SIGKILL)
                    proc.wait(timeout=TERMINATE_GRACE)
                except (OSError, subprocess.TimeoutExpired):
                    pass
                rc = 124
    finally:
        if holder in _ACTIVE:
            _ACTIVE.remove(holder)
    secs = int(time.time() - start)
    if rc != 0:
        register_row(crate, "compile", rc, secs, log_path)
        raise BenchError(f"compile failed for {crate}: rc={rc} log={log_path}")
    binaries = _resolve_binaries(crate, targets, log_path, start)
    log(f"compile {crate} done rc=0 {secs}s -> {len(binaries)} binaries")
    return binaries


def _resolve_binaries(crate: str, targets: list[tuple[str, bool]],
                      json_log: str, compile_start: float) -> list[tuple[str, str]]:
    """Canonical (name, path) pairs; two names globbing one file set = error."""
    json_found = _executables_from_json(json_log, crate)
    result: list[tuple[str, str]] = []
    claimed: dict[str, str] = {}  # path -> name that claimed it first
    for name, _harness in targets:
        path = json_found.get(name)
        if path is None or not os.path.isfile(path):
            # fallback glob, freshness-checked against compile start
            path = _find_built_binary(name, crate, compile_start)
            if os.path.getmtime(path) < compile_start - FRESHNESS_SLACK:
                raise BenchError(
                    f"stale glob fallback for bench {name!r} of {crate}: "
                    f"{path} predates compile start"
                )
        owner = claimed.get(path)
        if owner is not None:
            raise BenchError(
                f"benches {owner!r} and {name!r} of {crate} resolve to the "
                f"same binary {path}"
            )
        claimed[path] = name
        result.append((name, path))
    return result


def _pid_is_live(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _lease_occupied(lease: str) -> bool:
    """A lease whose pid file names a live foreign process is occupied.

    owner.pid records the WORKER pid (G5), so a lease whose worker exited
    reads as free even before the directory is removed."""
    pid_file = os.path.join(lease, "owner.pid")
    try:
        with open(pid_file) as fh:
            pid = int(fh.read().strip())
    except (OSError, ValueError):
        return False
    return _pid_is_live(pid)


def _claim_lease(cand: str) -> int | None:
    """TOCTOU-safe lease claim (G6): take an exclusive non-blocking flock on
    the lease dir's `.lock` file, then re-check occupancy under the lock.

    Returns the held lock fd (released by the caller after owner.pid names
    the worker), or None if the lease is taken/occupied."""
    os.makedirs(cand, exist_ok=True)
    lock_fd = os.open(os.path.join(cand, ".lock"),
                      os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        os.close(lock_fd)
        return None
    if _lease_occupied(cand):
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)
        return None
    return lock_fd


def measure_phase(crate: str, jobs: list[tuple[str, str]], resume: bool = False,
                  bench_args: list[str] | None = None) -> int:
    """Mode B: worker pool (max 4) over (name, binary) pairs; 0 iff all green.

    bench_args (G12) are forwarded verbatim to each binary after `--bench`.
    """
    _install_signal_handlers()
    if resume:
        queue = [(n, b) for n, b in jobs if not _bench_has_green(crate, n)]
        if len(queue) < len(jobs):
            log(f"measure {crate}: {len(jobs) - len(queue)} already green, skipped")
    else:
        queue = list(jobs)
    # queue entries: (bench, binary, retried)
    queue = [(n, b, False) for n, b in queue]
    reds = 0
    pending_reds: set[str] = set()  # benches awaiting the verdict of a retry
    pending_retries: list[tuple[str, str, bool]] = []
    workers: list[dict] = []
    leases: set[str] = set()
    terminating = False

    def free_lease() -> tuple[str, int]:
        live = {w["lease"] for w in workers}
        for i in range(1, MAX_WORKERS + 1):
            cand = os.path.join(LEASE_ROOT, f"purrdf-fast-t{i}")
            if cand in live:
                continue
            lock_fd = _claim_lease(cand)
            if lock_fd is not None:
                return cand, lock_fd
        raise BenchError("no free lease slot")

    def release_lease(lease: str) -> None:
        leases.discard(lease)
        shutil.rmtree(lease, ignore_errors=True)

    def _cleanup_children_and_leaves() -> None:
        nonlocal terminating
        if terminating:
            return
        terminating = True
        for w in workers:
            proc: subprocess.Popen = w["proc"]
            if proc.poll() is None:
                w["forced"] = True
                try:
                    _signal_group(proc, signal.SIGTERM)
                except OSError:
                    pass
        deadline = time.time() + TERMINATE_GRACE
        for w in workers:
            proc = w["proc"]
            remaining = max(0.0, deadline - time.time())
            try:
                proc.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                w["forced"] = True
                try:
                    _signal_group(proc, signal.SIGKILL)
                    proc.wait(timeout=TERMINATE_GRACE)
                except (OSError, subprocess.TimeoutExpired):
                    pass
        for lease in leases:
            shutil.rmtree(lease, ignore_errors=True)
        leases.clear()

    def spawn(bench: str, binary: str, retried: bool) -> None:
        # finding 12/13: without --bench the binary runs Test mode (no data,
        # exit 0); without PURRDF_BENCH_HOME the store is Unplaced (exit 101).
        _preflight_binary(crate, bench, binary)
        lease, lock_fd = free_lease()
        leases.add(lease)
        log_path = os.path.join(LOGS, f"{crate}.{bench}.fast.log")
        log(f"measure {crate}::{bench} starting (lease {lease}, pid spawn)")
        log_fh = open(log_path, "wb")
        env = build_env(lease)
        env["PURRDF_BENCH_HOME"] = BENCH_STORE
        worker: dict = {
            "bench": bench, "binary": binary, "lease": lease,
            "start": time.time(), "retried": retried, "log_fh": log_fh,
            "proc": None, "cap": False, "forced": False, "entry": None,
        }
        # G9: register with the signal registry BEFORE the spawn so a signal
        # landing inside the spawn window cannot miss this worker.
        entry: dict = {"proc": None}
        _ACTIVE.append(entry)
        worker["entry"] = entry
        workers.append(worker)
        proc = subprocess.Popen(
            _bench_cmd(binary, bench_args), stdout=log_fh,
            stderr=subprocess.STDOUT, env=env, start_new_session=True,
        )
        worker["proc"] = proc
        entry["proc"] = proc
        # G5: owner.pid records the WORKER pid (not the orchestrator's),
        # written under the lease lock claimed in free_lease.
        with open(os.path.join(lease, "owner.pid"), "w") as fh:
            fh.write(f"{proc.pid}\n")
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)

    def drain(w: dict) -> None:
        """Record one finished worker's row; per-bench reds accounting (G4):
        a killed first attempt parks the bench in pending_reds; a green
        retry unparks it, a red retry (or any other red) counts once."""
        nonlocal reds
        raw = w["proc"].returncode
        rc = _normalize_rc(raw, w.get("cap", False))
        assert rc is not None, "register rc can never be None (G3)"
        assert not _killed(raw) or rc in (124, 137), \
            f"signal death must normalize to 137/124, got raw={raw!r} rc={rc!r}"
        secs = int(time.time() - w["start"])
        log_path = os.path.join(LOGS, f"{crate}.{w['bench']}.fast.log")
        register_row(crate, w["bench"], rc, secs, log_path, retried=w["retried"])
        log(f"measure {crate}::{w['bench']} rc={rc} {secs}s"
            + (" retry=1" if w["retried"] else ""))
        try:
            w["log_fh"].close()
        except OSError:
            pass
        if rc == 101:
            kind = classify_rc101(crate, w["bench"], log_path)
            log(f"measure {crate}::{w['bench']} rc=101 class={kind} "
                f"(failed=footer count / refused=error: line)")
        entry = w.get("entry")
        if entry is not None and entry in _ACTIVE:
            _ACTIVE.remove(entry)
        if rc == 0:
            pending_reds.discard(w["bench"])
        elif rc == 137 and not w["retried"]:
            pending_reds.add(w["bench"])  # verdict deferred to the retry
        else:
            pending_reds.discard(w["bench"])
            reds += 1

    def _poll_runnable(w: dict) -> bool:
        """Bounded CAP per run: poll; on expiry kill and report rc=124."""
        proc = w["proc"]
        if proc.poll() is not None:
            return True
        if time.time() - w["start"] > CAP:
            log(f"measure {crate}::{w['bench']} exceeded CAP={CAP}s; killing")
            w["cap"] = True
            try:
                _signal_group(proc, signal.SIGKILL)
            except OSError:
                pass
            try:
                proc.wait(timeout=TERMINATE_GRACE)
            except subprocess.TimeoutExpired:
                pass
            return True
        return False

    atexit.register(_cleanup_children_and_leaves)
    try:
        queue = list(queue)
        while queue or workers or pending_retries:
            # G2: a retry spawns only once the pool has drained to EMPTY —
            # the retry is provably the only bench in flight. Spawned
            # directly (not requeued) so it can never be reordered behind a
            # queued fresh bench.
            if pending_retries and not workers:
                bench_r, binary_r, retried_r = pending_retries.pop(0)
                spawn(bench_r, binary_r, retried_r)
            solo_in_flight = (
                bool(pending_retries)
                or any(w["retried"] for w in workers))
            while (queue and not solo_in_flight
                   and len(workers) < MAX_WORKERS):
                spawn(*queue.pop(0))
            time.sleep(POLL)
            for w in [w for w in workers if _poll_runnable(w)]:
                workers.remove(w)
                raw = w["proc"].returncode
                if _killed(raw) and not w.get("cap") and not w["retried"]:
                    # Record the SIGKILL'd attempt as its own row BEFORE the
                    # solo retry; the retry gets a second, retry-marked row.
                    drain(w)
                    release_lease(w["lease"])
                    log(f"measure {crate}::{w['bench']} rc=137 (SIGKILL); "
                        "retry once solo (pool drained to empty first)")
                    pending_retries.append((w["bench"], w["binary"], True))
                else:
                    drain(w)
    finally:
        for w in list(workers):  # stragglers on error paths: bounded wait
            try:
                w["proc"].wait(timeout=TERMINATE_GRACE)
            except subprocess.TimeoutExpired:
                w["forced"] = True
                try:
                    _signal_group(w["proc"], signal.SIGKILL)
                    w["proc"].wait(timeout=TERMINATE_GRACE)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            drain(w)
            workers.remove(w)
        for lease in leases:  # lease law: deleted at exit
            shutil.rmtree(lease, ignore_errors=True)
        leases.clear()
    # a parked bench whose retry never ran is still red — exit honest (G4)
    reds += len(pending_reds)
    return 1 if reds else 0


def _bench_has_green(crate: str, bench: str) -> bool:
    prefix = f"{crate} BENCH={bench} "
    return any(
        line.startswith(prefix) and " rc=0 " in line
        for line in _locked_register_lines()
    )


def compiled_binaries(crate: str, allow_stale: bool = False) -> list[tuple[str, str]]:
    """Re-resolve (name, path) pairs from a prior compile phase (measure
    standalone). JSON resolution by canonical manifest name is authoritative.
    G15: when a bench cannot be resolved from the compile log, this mode has
    no freshness gate to lean on — refuse with a typed BenchError unless the
    operator passed --allow-stale-binaries explicitly (the glob fallback then
    applies with no compile-start gate, i.e. it may be stale)."""
    manifest = crate_manifest(crate)
    targets = bench_names(manifest)
    log_path = os.path.join(LOGS, f"{crate}.compile.fast.log")
    json_found = (
        _executables_from_json(log_path, crate)
        if os.path.exists(log_path) else {}
    )
    result: list[tuple[str, str]] = []
    claimed: dict[str, str] = {}
    for name, _harness in targets:
        path = json_found.get(name)
        if path is None or not os.path.isfile(path):
            if not allow_stale:
                raise BenchError(
                    f"bench {name!r} of {crate} has no binary from a compile "
                    f"phase (no JSON executable in {log_path}); run "
                    f"`compile {crate}` first, or pass --allow-stale-binaries "
                    f"to accept a possibly-stale deps-glob binary"
                )
            path = _find_built_binary(name, crate, 0.0)
        owner = claimed.get(path)
        if owner is not None:
            raise BenchError(
                f"benches {owner!r} and {name!r} of {crate} resolve to the "
                f"same binary {path}"
            )
        claimed[path] = name
        result.append((name, path))
    return result


def _baseline_flag_values(args: argparse.Namespace) -> list[str]:
    return [v for v in (args.save_baseline, args.baseline,
                        args.baseline_lenient) if v]


def _bench_forward_args(args: argparse.Namespace) -> list[str]:
    """Passthrough flags forwarded after `--bench` (G12); validates the
    exclusive baseline group for direct callers (argparse enforces it for
    CLI callers)."""
    conflict = [v for v in (args.save_baseline, args.baseline,
                            args.baseline_lenient) if v]
    if len(conflict) > 1:
        raise BenchError(
            "--save-baseline/--baseline/--baseline-lenient are mutually "
            f"exclusive; got {conflict}")
    out: list[str] = []
    if args.quick:
        out.append("--quick")
    if args.exact:
        out.append("--exact")
    for f in args.filter:
        out += ["--filter", f]
    if args.save_baseline:
        out += ["--save-baseline", args.save_baseline]
    elif args.baseline:
        out += ["--baseline", args.baseline]
    elif args.baseline_lenient:
        out += ["--baseline-lenient", args.baseline_lenient]
    return out


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="run_benches_fast.py",
        description="Fast two-phase bench orchestrator (compile then measure).",
    )
    parser.add_argument("mode", choices=["compile", "measure", "run"],
                        help="compile (Mode A), measure (Mode B), or both")
    parser.add_argument("--resume", action="store_true",
                        help="skip crates with green register rows")
    parser.add_argument("--quick", action="store_true",
                        help="forward --quick to each bench binary")
    parser.add_argument("--exact", action="store_true",
                        help="forward --exact to each bench binary")
    parser.add_argument("--filter", action="append", default=[],
                        metavar="F",
                        help="forward --filter F to each bench binary "
                             "(repeatable)")
    baseline = parser.add_mutually_exclusive_group()
    baseline.add_argument("--save-baseline", metavar="NAME",
                          help="forward --save-baseline NAME (exclusive)")
    baseline.add_argument("--baseline", metavar="NAME",
                          help="forward --baseline NAME (exclusive)")
    baseline.add_argument("--baseline-lenient", metavar="NAME",
                          help="forward --baseline-lenient NAME (exclusive)")
    parser.add_argument("--allow-stale-binaries", action="store_true",
                        help="standalone `measure` only: accept deps-glob "
                             "binaries with no freshness gate instead of "
                             "refusing; run `compile` first otherwise")
    parser.add_argument("crates", nargs="+", metavar="CRATE")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    bench_args = _bench_forward_args(args)

    os.makedirs(LOGS, exist_ok=True)
    crates = list(args.crates)
    if args.resume:
        kept = []
        for c in crates:
            targets = bench_names(crate_manifest(c))
            if crate_has_green_row(c, targets):
                log(f"--resume: {c} fully green (all {len(targets)} benches), skipped")
            else:
                kept.append(c)
        crates = kept
        if not crates:
            log("--resume: all requested crates already green")
            return 0

    log(f"mode={args.mode} crates={crates} bench_args={bench_args}")
    if args.mode in ("compile", "run"):
        for crate in crates:
            compile_phase(crate)
    if args.mode in ("measure", "run"):
        for crate in crates:
            jobs = (compiled_binaries(crate, allow_stale=args.allow_stale_binaries)
                    if args.mode == "measure" else compile_phase(crate))
            if measure_phase(crate, jobs, resume=args.resume,
                             bench_args=bench_args) != 0:
                log(f"measure {crate}: RED")
                return 1
    log("complete")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BenchError as exc:
        print(f"[bench-fast] ERROR: {exc}", file=sys.stderr, flush=True)
        sys.exit(2)
