"""Session fixture: load the pinned corpus and verify every blake3 digest.

The digest check is the pin: any hand-edit of vectors/*.json (or a generator
drift) fails loudly here before a single case is evaluated.

Runtime budget enforcement (lane C8)
------------------------------------
Every test's ``call`` phase is timed and checked against budgets:

* per-test default budget: 10 ms  (env: ``ORACLE_TEST_BUDGET_MS``)
* session cumulative budget: 1.0 s (env: ``ORACLE_SESSION_BUDGET_MS``),
  measured as the ACCUMULATED per-test call durations (lazy-clock equivalent:
  corpus load and collection never enter the budget by construction)

A test exceeding its budget has its report failed with
``ORACLE-OVERBUDGET[10ms]: <test> took <t>ms``; a session exceeding its total
fails at session end with ``ORACLE-SESSION-OVERBUDGET[1.0s]: total <t>ms``.
Every session end emits an ``ORACLE-RECEIPT`` line (oracle/case counts read
from vectors/manifest.json + total vs budget + verdict) BEFORE any
over-budget SystemExit, so a refusal is never stdout-only.
A test with a legitimately high runtime may carry a per-test marker
``@pytest.mark.oracle_budget(ms=N)`` which the hook honors instead of the
default (never silently raise the global budget); the marker is typed —
an ``oracle_budget`` without ``ms=`` refuses at configuration time.

pm4pytest note: the optional ``pm4pytest`` process-intelligence plugin is
attempted at import time; when the package is absent (current state on this
machine — ``No module named 'pm4pytest'``) the suite proceeds without it. This
is a typed skip note, not a failure. If installed, the plugin is registered
via the pytest plugin manager (``consider_module``) so its ``pm4py_session`` /
``ocpq_session`` fixtures and gate-enforcing ``pytest_runtest_makereport``
hookwrappers are active.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from collections.abc import Generator

import blake3
import pytest

ROOT = Path(__file__).resolve().parent
VECTORS = ROOT / "vectors"
MODULES = ["bitset", "swar", "eytzinger", "sort_small", "packing"]

# --- budget configuration (operator defaults, env-overridable) -------------
DEFAULT_TEST_BUDGET_MS = 10.0
DEFAULT_SESSION_BUDGET_MS = 1000.0


def _budget_from_env(var: str, default_ms: float) -> float:
    """Read a budget override from *var*, refusing malformed values loudly."""
    raw = os.environ.get(var)
    if raw is None:
        return default_ms
    try:
        value = float(raw)
    except ValueError:
        raise SystemExit(
            f"ORACLE-BUDGET-CONFIG: {var}={raw!r} is not a number"
        ) from None
    if value <= 0:
        raise SystemExit(f"ORACLE-BUDGET-CONFIG: {var}={raw!r} must be > 0 ms")
    return value


# --- optional pm4pytest plugin registration ---------------------------------
_PM4PYTEST_STATUS = "absent (proceeding without it)"
try:  # typed skip: optional dependency, absence is recorded, never raised
    import pm4pytest  # noqa: F401

    _PM4PYTEST_STATUS = "present"
except ImportError:
    pm4pytest = None  # type: ignore[assignment]


def pytest_configure(config: pytest.Config) -> None:
    """Register the oracle_budget marker and the optional pm4pytest plugin."""
    config.addinivalue_line(
        "markers",
        "oracle_budget(ms=N): per-test runtime budget in ms, overriding the "
        "default ORACLE-OVERBUDGET enforcement for this test.",
    )
    if pm4pytest is not None:
        config.pluginmanager.consider_module(pm4pytest)


# --- timing state ------------------------------------------------------------
_SESSION_TOTAL_MS = 0.0


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(
    item: pytest.Item, call: pytest.CallInfo[None]
) -> Generator[None, pytest.TestReport, None]:  # type: ignore[name-defined]
    """Time each test's call phase and fail over-budget reports (typed)."""
    outcome = yield
    if call.when != "call":
        return

    report = outcome.get_result()
    elapsed_ms = report.duration * 1000.0
    global _SESSION_TOTAL_MS
    _SESSION_TOTAL_MS += elapsed_ms

    marker = item.get_closest_marker("oracle_budget")
    if marker is not None:
        if "ms" not in marker.kwargs:
            raise SystemExit(
                "ORACLE-BUDGET-CONFIG: oracle_budget marker requires ms=N")
        budget_ms = float(marker.kwargs["ms"])
    else:
        budget_ms = _budget_from_env("ORACLE_TEST_BUDGET_MS", DEFAULT_TEST_BUDGET_MS)

    if elapsed_ms > budget_ms and report.outcome != "failed":
        report.outcome = "failed"
        report.longrepr = (
            f"ORACLE-OVERBUDGET[{budget_ms:g}ms]: "
            f"{item.nodeid} took {elapsed_ms:.3f}ms"
        )
    elif elapsed_ms > budget_ms:
        # test already failed; still record the budget breach next to it
        report.sections.append(
            (
                "ORACLE-OVERBUDGET",
                f"ORACLE-OVERBUDGET[{budget_ms:g}ms]: "
                f"{item.nodeid} took {elapsed_ms:.3f}ms",
            )
        )


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> int:
    """Enforce the accumulated per-test budget and emit the typed receipt.

    The session total is the ACCUMULATED per-test call durations, not a wall
    clock: corpus load and collection drop out of the budget by construction
    (lazy-clock equivalent). The ORACLE-RECEIPT line is emitted on BOTH the
    pass and over-budget paths, before any SystemExit, so the refusal is
    never stdout-only.
    """
    manifest = json.loads((VECTORS / "manifest.json").read_text())
    n_oracles = len(manifest["files"])
    n_cases = sum(m["cases"] for m in manifest["files"].values())
    total_ms = _SESSION_TOTAL_MS
    budget_ms = _budget_from_env("ORACLE_SESSION_BUDGET_MS", DEFAULT_SESSION_BUDGET_MS)
    over = total_ms > budget_ms
    terminal = session.config.get_terminal_writer()
    terminal.line("")
    terminal.line(
        f"ORACLE-RECEIPT: oracles={n_oracles} cases={n_cases} "
        f"total_ms={total_ms:.3f} budget_ms={budget_ms:g} "
        f"verdict={'FAIL' if over else 'PASS'}"
    )
    if over:
        terminal.sep("=", red=True)
        terminal.line(
            f"ORACLE-SESSION-OVERBUDGET[{budget_ms / 1000.0:g}s]: "
            f"total {total_ms:.3f}ms",
            red=True,
        )
        terminal.sep("=", red=True)
        # pytest ignores the hook's return value for the process exit code,
        # so the over-budget refusal is raised explicitly (fail-closed).
        raise SystemExit(1)
    terminal.line(
        f"ORACLE-BUDGET: per-test budgets enforced; accumulated call total "
        f"{total_ms:.3f}ms of {budget_ms / 1000.0:g}s budget "
        f"(pm4pytest: {_PM4PYTEST_STATUS})"
    )
    return exitstatus


# --- pinned corpus fixture ---------------------------------------------------
@pytest.fixture(scope="session")
def corpus(request):
    """Yields {module: {fn: [case, ...]}} after verifying the manifest pin."""
    manifest = json.loads((VECTORS / "manifest.json").read_text())
    assert manifest["schema"] == 1, "manifest schema drift"
    assert sorted(manifest["files"]) == sorted(MODULES), (
        f"manifest files drift: {sorted(manifest['files'])}")

    corpus: dict[str, dict[str, list[dict]]] = {}
    for module in MODULES:
        path = VECTORS / f"{module}.json"
        raw = path.read_bytes()
        want = manifest["files"][module]
        digest = blake3.blake3(raw).hexdigest()
        assert digest == want["blake3"], (
            f"VECTOR PIN DRIFT: vectors/{module}.json blake3 {digest} != pinned "
            f"{want['blake3']} — regenerate with `python3 -m gen.gen_vectors`")
        doc = json.loads(raw)
        assert doc["schema"] == 1
        assert doc["module"] == module
        assert len(doc["cases"]) == want["cases"], (
            f"case count drift in vectors/{module}.json")
        # index by fn
        by_fn: dict[str, list[dict]] = {}
        for case in doc["cases"]:
            assert set(case) == {"id", "fn", "in", "expect"}, f"bad case keys: {case.get('id')}"
            by_fn.setdefault(case["fn"], []).append(case)
        corpus[module] = by_fn

    yield corpus
