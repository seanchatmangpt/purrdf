#!/usr/bin/env python3
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
                       harness=false plain executables.
  Mode B (measure)   : worker pool (max 4 concurrent) runs each bench binary
                       directly, each worker with its own
                       CARGO_TARGET_DIR=/tmp/purrdf-fast-t<N> lease (created
                       on start, deleted at exit — lease law; a lease holding
                       a live foreign pid is occupied), output to
                       /tmp/bench-logs/<crate>.<bench>.fast.log, one register
                       row `CRATE BENCH=NAME rc=R SECS=S log=...` appended
                       under an exclusive flock to
                       /tmp/bench-logs/register-fast.txt via a single
                       O_APPEND write. rc=137 (SIGKILL) records the killed
                       attempt as its own row, then is retried once with no
                       other bench in flight (solo); the retry gets a second
                       row marked ` retry=1`. Each run is capped at CAP
                       seconds (killed -> rc=124 row).

  `--resume` skips a crate only when EVERY bench name from its manifest has
  a green (rc=0) register row; without --resume no green row skips any
  re-measurement. SIGTERM/SIGINT/SIGHUP terminate children (terminate,
  bounded 5s wait, then kill), remove all leases and exit non-zero; an
  atexit backstop repeats the lease cleanup. The only sleep is the 0.2s
  worker poll (bounded, on a real observable: Popen.poll).

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


class BenchError(RuntimeError):
    """Typed orchestrator failure (missing manifest, compile red, no binary)."""


def log(msg: str) -> None:
    line = f"[bench-fast] {msg}"
    print(line, flush=True)
    with open(RUNNER_LOG, "a") as fh:
        fh.write(line + "\n")


def register_row(crate: str, bench: str, rc: int, secs: int, path: str,
                 retried: bool = False) -> None:
    """Single atomic O_APPEND write under an exclusive register flock."""
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
    if shutil.which("sccache"):
        env["RUSTC_WRAPPER"] = "sccache"
    else:
        env.pop("RUSTC_WRAPPER", None)
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
    """`<binary> --list` must exit 0 (touches no store) before the real run."""
    probe = subprocess.run(
        [binary, "--list"], stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, env=build_env(lease_env_dir(bench)),
        timeout=60,
    )
    if probe.returncode != 0:
        raise BenchError(
            f"pre-flight --list failed for bench {bench!r} of {crate}: "
            f"rc={probe.returncode} binary={binary}"
        )


def lease_env_dir(bench: str) -> str:
    """Store home for pre-flight probes; the worker lease is set at spawn."""
    return f"/tmp/purrdf-fast-probe-{bench}"


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
    only and requires freshness (mtime postdating compile start).
    """
    manifest = crate_manifest(crate)
    crate_dir = os.path.dirname(manifest)
    targets = bench_names(manifest)
    names = [n for n, _h in targets]
    log(f"compile {crate} starting ({len(names)} bench targets: {', '.join(names)})")
    log_path = os.path.join(LOGS, f"{crate}.compile.fast.log")
    start = time.time()
    try:
        with open(log_path, "wb") as fh:
            proc = subprocess.run(
                ["cargo", "bench", "-p", crate, "--no-run",
                 "--message-format=json"],
                cwd=crate_dir, stdout=fh, stderr=subprocess.STDOUT,
                env=build_env(SHARED_TARGET), timeout=CAP,
            )
        rc = proc.returncode
    except subprocess.TimeoutExpired:
        rc = 124
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
    """A lease whose pid file names a live foreign process is occupied."""
    pid_file = os.path.join(lease, "owner.pid")
    try:
        with open(pid_file) as fh:
            pid = int(fh.read().strip())
    except (OSError, ValueError):
        return False
    return _pid_is_live(pid)


def measure_phase(crate: str, jobs: list[tuple[str, str]], resume: bool = False) -> int:
    """Mode B: worker pool (max 4) over (name, binary) pairs; 0 iff all green."""
    if resume:
        queue = [(n, b) for n, b in jobs if not _bench_has_green(crate, n)]
        if len(queue) < len(jobs):
            log(f"measure {crate}: {len(jobs) - len(queue)} already green, skipped")
    else:
        queue = list(jobs)
    reds = 0
    workers: list[dict] = []
    leases: set[str] = set()
    terminating = False

    def free_lease() -> str:
        live = {w["lease"] for w in workers}
        for i in range(1, MAX_WORKERS + 1):
            cand = f"/tmp/purrdf-fast-t{i}"
            if cand in live:
                continue
            if os.path.isdir(cand) and _lease_occupied(cand):
                continue
            return cand
        raise BenchError("no free lease slot")

    def _cleanup_children_and_leaves() -> None:
        nonlocal terminating
        if terminating:
            return
        terminating = True
        for w in workers:
            proc: subprocess.Popen = w["proc"]
            if proc.poll() is None:
                try:
                    proc.terminate()
                except OSError:
                    pass
        deadline = time.time() + TERMINATE_GRACE
        for w in workers:
            proc = w["proc"]
            remaining = max(0.0, deadline - time.time())
            try:
                proc.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                try:
                    proc.kill()
                    proc.wait(timeout=TERMINATE_GRACE)
                except (OSError, subprocess.TimeoutExpired):
                    pass
        for lease in leases:
            shutil.rmtree(lease, ignore_errors=True)
        leases.clear()

    def _on_signal(signum, _frame):  # noqa: ANN001 — signal handler signature
        log(f"signal {signum}: terminating children and releasing leases")
        _cleanup_children_and_leaves()
        sys.exit(128 + signum)

    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, _on_signal)
    atexit.register(_cleanup_children_and_leaves)

    def spawn(bench: str, binary: str) -> None:
        # finding 12/13: without --bench the binary runs Test mode (no data,
        # exit 0); without PURRDF_BENCH_HOME the store is Unplaced (exit 101).
        _preflight_binary(crate, bench, binary)
        lease = free_lease()
        os.makedirs(lease, exist_ok=True)
        with open(os.path.join(lease, "owner.pid"), "w") as fh:
            fh.write(f"{os.getpid()}\n")
        leases.add(lease)
        log_path = os.path.join(LOGS, f"{crate}.{bench}.fast.log")
        log(f"measure {crate}::{bench} starting (lease {lease}, pid spawn)")
        log_fh = open(log_path, "wb")
        env = build_env(lease)
        env["PURRDF_BENCH_HOME"] = BENCH_STORE
        workers.append({
            "bench": bench, "binary": binary, "lease": lease,
            "start": time.time(), "retried": False, "log_fh": log_fh,
            "proc": subprocess.Popen(
                [binary, "--bench"], stdout=log_fh,
                stderr=subprocess.STDOUT, env=env,
            ),
        })

    def drain(w: dict) -> None:
        """Record one finished worker's row; counts reds."""
        nonlocal reds
        rc = w["proc"].returncode
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
        if rc != 0:
            reds += 1

    def _poll_runnable(w: dict) -> bool:
        """Bounded CAP per run: poll; on expiry kill and report rc=124."""
        proc = w["proc"]
        if proc.poll() is not None:
            return True
        if time.time() - w["start"] > CAP:
            log(f"measure {crate}::{w['bench']} exceeded CAP={CAP}s; killing")
            try:
                proc.kill()
            except OSError:
                pass
            try:
                proc.wait(timeout=TERMINATE_GRACE)
            except subprocess.TimeoutExpired:
                pass
            return True
        return False

    try:
        queue = list(queue)
        while queue or workers:
            # rc=137 solo retry: while a retry is in flight, no fresh spawns —
            # the retry must be the only bench running.
            solo_in_flight = any(w["retried"] for w in workers)
            while (queue and not solo_in_flight
                   and len(workers) < MAX_WORKERS):
                spawn(*queue.pop(0))
            time.sleep(POLL)
            for w in [w for w in workers if _poll_runnable(w)]:
                workers.remove(w)
                if w["proc"].returncode == 137 and not w["retried"]:
                    # Record the SIGKILL'd attempt as its own row BEFORE the
                    # solo retry; the retry gets a second, retry-marked row.
                    drain(w)
                    log(f"measure {crate}::{w['bench']} rc=137 (SIGKILL); "
                        "retry once solo (no other bench in flight)")
                    w["retried"] = True
                    w["start"] = time.time()
                    w["log_fh"] = open(
                        os.path.join(LOGS, f"{crate}.{w['bench']}.fast.log"), "wb")
                    retry_env = build_env(w["lease"])
                    retry_env["PURRDF_BENCH_HOME"] = BENCH_STORE
                    w["proc"] = subprocess.Popen(
                        [w["binary"], "--bench"],
                        stdout=w["log_fh"],
                        stderr=subprocess.STDOUT,
                        env=retry_env,
                    )
                    workers.append(w)
                else:
                    drain(w)
    finally:
        for w in list(workers):  # stragglers on error paths: bounded wait
            try:
                w["proc"].wait(timeout=TERMINATE_GRACE)
            except subprocess.TimeoutExpired:
                try:
                    w["proc"].kill()
                    w["proc"].wait(timeout=TERMINATE_GRACE)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            drain(w)
            workers.remove(w)
        for lease in leases:  # lease law: deleted at exit
            shutil.rmtree(lease, ignore_errors=True)
        leases.clear()
    return 1 if reds else 0


def _bench_has_green(crate: str, bench: str) -> bool:
    prefix = f"{crate} BENCH={bench} "
    return any(
        line.startswith(prefix) and " rc=0 " in line
        for line in _locked_register_lines()
    )


def compiled_binaries(crate: str) -> list[tuple[str, str]]:
    """Re-resolve (name, path) pairs from a prior compile phase (measure
    standalone). No compile start is known, so neither the JSON resolution
    nor the glob fallback carries a freshness gate in this mode; JSON
    resolution by canonical manifest name is authoritative here."""
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="run_benches_fast.py",
        description="Fast two-phase bench orchestrator (compile then measure).",
    )
    parser.add_argument("mode", choices=["compile", "measure", "run"],
                        help="compile (Mode A), measure (Mode B), or both")
    parser.add_argument("--resume", action="store_true",
                        help="skip crates with green register rows")
    parser.add_argument("crates", nargs="+", metavar="CRATE")
    args = parser.parse_args(argv)

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

    log(f"mode={args.mode} crates={crates}")
    if args.mode in ("compile", "run"):
        for crate in crates:
            compile_phase(crate)
    if args.mode in ("measure", "run"):
        for crate in crates:
            jobs = (compiled_binaries(crate) if args.mode == "measure"
                    else compile_phase(crate))
            if measure_phase(crate, jobs, resume=args.resume) != 0:
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
