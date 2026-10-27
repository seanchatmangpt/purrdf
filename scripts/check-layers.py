#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

"""Hold the first-party crate graph to the layering ``layers.toml`` declares.

A shared helper can only live where every caller reaches it, so the crate graph
decides where one implementation of a job may go. Left undeclared, that graph
grows by whatever edge was convenient on the day, and the lowest common crate of
two callers drifts upward until the only place a helper fits is a crate that
half its callers must not depend on. ``layers.toml`` is the declaration; this gate
compares it with the resolved graph — static manifest resolution by default,
``cargo metadata`` under ``--via-cargo`` — and fails on:

* a first-party NORMAL edge no row allows (an edge to the declared root is
  always allowed, and is listed like any other so the table stays exact);
* a workspace member without a row, or a row (not ``pending``) that names no
  member;
* a listed edge the resolved graph no longer has, and a ``pending`` edge or
  crate that now exists — the table records the graph exactly, so a planned edge
  is flipped to present in the change that adds it;
* a cycle in the declared graph, pending edges included;
* a crate in ``FORBID_UNSAFE_CRATES`` whose ``src/lib.rs`` no longer carries the
  crate-level ``#![forbid(unsafe_code)]`` (those crates hold no ``unsafe`` and
  must not be able to grow one silently).

``--ring-fence`` (``make rdf-core-hygiene``) holds the kernel ring-fence, which
counts EVERY normal dependency, external crates included: the root has none at
all, and a row carrying ``external`` is ring-fenced — each of its normal
dependencies is one of its listed first-party ``deps`` or one of the external
packages ``external`` names, and a listed external it no longer has is STALE.

The DEFAULT mode is the exact static resolver: it reads every member manifest's
``[dependencies]`` and ``[target.*.dependencies]`` sections (dev and build
dependencies are excluded — layering is normal-only), resolves first-party
entries the way cargo does (workspace aliases, ``package =`` renames, path
dependencies into members), intersects with the member set exactly as the
cargo path does, and diffs the edge set against ``layers.toml`` — no cargo
subprocess, no Cargo.lock, the whole gate in tens of milliseconds. The only
things it cannot see live in features and workspace resolver behavior; the
``--via-cargo`` flag runs the identical diff through the authoritative
``cargo metadata`` graph, asserts the static and cargo member and edge sets
are IDENTICAL (printing both sides of any symmetric difference), and prints
both counts — the standing cross-check that keeps the static resolver honest.

``--fast`` is a NECESSARY-NOT-SUFFICIENT pre-check that touches no Cargo.lock
and runs no cargo at all (a sub-200ms developer loop, not a gate). It reads
``layers.toml`` and each ``crates/*/Cargo.toml`` top-level ``[dependencies]``
section only — target tables, dev/build sections and rename subtleties are its
known blind spots, by design — and requires the declared set and the row's
present edges to be exactly equal, both ways.

``--home-for A B …`` answers the placement question: over the declared graph
(pending edges included), the crates every one of ``A``, ``B``, … reaches,
directly or transitively (each crate reaches itself), minus any that another of
them reaches — the common dependency closest to the callers. When the callers
share nothing else it is the root, which every crate may depend on.

The ``--via-cargo`` and ``--ring-fence`` graphs come from
``check-banned-deps.py``'s ``cargo_metadata`` and ``graph_from_metadata``; this
script imports them rather than restating them.
"""

from __future__ import annotations

import importlib.util
import sys
from time import perf_counter
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from types import ModuleType

REPO_ROOT = Path(__file__).resolve().parent.parent
LAYERS_PATH = REPO_ROOT / "layers.toml"

# Crates that hold no `unsafe` and must say so: directory under crates/.
FORBID_UNSAFE_CRATES = ("rdf", "gts", "hnsw", "shapes", "slice", "purrdf")
FORBID_UNSAFE_ATTRIBUTE = "#![forbid(unsafe_code)]"


def load_banned_deps() -> ModuleType:
    """``scripts/check-banned-deps.py`` as a module (its name has a hyphen)."""
    path = Path(__file__).resolve().parent / "check-banned-deps.py"
    spec = importlib.util.spec_from_file_location("check_banned_deps", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules.setdefault("check_banned_deps", module)
    spec.loader.exec_module(module)
    return module


BANNED_DEPS = load_banned_deps()


class LayersError(ValueError):
    """``layers.toml`` is malformed; the message says where."""


@dataclass
class Layers:
    """The declared table."""

    root: str
    # crate -> {dependency -> pending}
    deps: dict[str, dict[str, bool]] = field(default_factory=dict)
    pending_crates: set[str] = field(default_factory=set)
    # ring-fenced crate -> the external packages it may depend on
    external: dict[str, set[str]] = field(default_factory=dict)


def parse_layers(document: dict) -> Layers:
    """Validate and read a parsed ``layers.toml``."""
    unknown = set(document) - {"root", "crate"}
    if unknown:
        raise LayersError(f"unknown top-level keys: {sorted(unknown)}")
    root = document.get("root")
    if not isinstance(root, str) or not root:
        raise LayersError("`root` must name a crate")
    rows = document.get("crate")
    if not isinstance(rows, list) or not rows:
        raise LayersError("there must be [[crate]] rows")
    layers = Layers(root=root)
    for row in rows:
        if not isinstance(row, dict):
            raise LayersError("a [[crate]] row must be a table")
        extra = set(row) - {"name", "pending", "deps", "external"}
        if extra:
            raise LayersError(f"row {row.get('name')!r}: unknown keys {sorted(extra)}")
        name = row.get("name")
        if not isinstance(name, str) or not name:
            raise LayersError("every row needs a `name`")
        if name in layers.deps:
            raise LayersError(f"`{name}` has two rows")
        pending = row.get("pending", False)
        if pending is not True and "pending" in row:
            raise LayersError(f"`{name}`: `pending` is `true` or absent")
        if pending:
            layers.pending_crates.add(name)
        entries = row.get("deps")
        if not isinstance(entries, list):
            raise LayersError(f"`{name}` needs a `deps` array")
        declared: dict[str, bool] = {}
        for entry in entries:
            if isinstance(entry, str):
                dep, dep_pending = entry, False
            elif isinstance(entry, dict) and set(entry) == {"name", "pending"}:
                dep, dep_pending = entry["name"], entry["pending"]
                if dep_pending is not True:
                    raise LayersError(f"`{name}` -> `{dep}`: write a present edge as a plain name")
            else:
                raise LayersError(f"`{name}`: a dep is a name or {{ name, pending = true }}")
            if dep in declared:
                raise LayersError(f"`{name}` lists `{dep}` twice")
            if dep == name:
                raise LayersError(f"`{name}` lists itself")
            declared[dep] = dep_pending
        layers.deps[name] = declared
        if "external" in row:
            external = row["external"]
            if not isinstance(external, list) or not all(isinstance(item, str) and item for item in external):
                raise LayersError(f"`{name}`: `external` is an array of package names")
            if len(set(external)) != len(external):
                raise LayersError(f"`{name}`: `external` lists a package twice")
            if name == root:
                raise LayersError(f"the root `{name}` is ring-fenced to nothing; it takes no `external`")
            layers.external[name] = set(external)
    if root not in layers.deps:
        raise LayersError(f"the root `{root}` needs a row")
    if layers.deps[root]:
        raise LayersError(f"the root `{root}` must have no dependencies")
    for name, declared in layers.deps.items():
        for dep in declared:
            if dep not in layers.deps:
                raise LayersError(f"`{name}` lists `{dep}`, which has no row")
    return layers


def resolved_edges(graph) -> tuple[set[str], set[tuple[str, str]]]:
    """(member names, first-party normal edges) of a ``ResolvedGraph``."""
    members = {graph.names[pid] for pid in graph.workspace}
    edges = {
        (graph.names[pid], graph.names[dep])
        for pid in graph.workspace
        for dep, kinds in graph.edges.get(pid, ())
        if dep in graph.workspace and "normal" in kinds
    }
    return members, edges


def cycle(layers: Layers) -> list[str] | None:
    """One cycle in the declared graph, or ``None``."""
    state: dict[str, int] = {}
    stack: list[str] = []

    def visit(name: str) -> list[str] | None:
        state[name] = 1
        stack.append(name)
        for dep in sorted(layers.deps[name]):
            if state.get(dep) == 1:
                return stack[stack.index(dep) :] + [dep]
            if dep not in state:
                found = visit(dep)
                if found:
                    return found
        stack.pop()
        state[name] = 2
        return None

    for name in sorted(layers.deps):
        if name not in state:
            found = visit(name)
            if found:
                return found
    return None


def failures(layers: Layers, members: set[str], edges: set[tuple[str, str]]) -> list[str]:
    """Every disagreement between the table and the resolved graph."""
    found: list[str] = []
    for name in sorted(members - set(layers.deps)):
        found.append(f"FAIL: workspace member `{name}` has no [[crate]] row in layers.toml")
    for name in sorted(set(layers.deps) - members - layers.pending_crates):
        found.append(
            f"FAIL: layers.toml row `{name}` names no workspace member (STALE); "
            "remove it or mark it `pending = true`"
        )
    for name in sorted(layers.pending_crates & members):
        found.append(
            f"FAIL: `{name}` is a workspace member now; drop `pending = true` from its row"
        )
    for source, target in sorted(edges):
        declared = layers.deps.get(source, {})
        if target in declared:
            continue
        if target == layers.root:
            found.append(
                f"FAIL: `{source}` -> `{target}` is not listed; an edge to the root is always "
                f"allowed, so add it to `{source}`'s row to keep the table exact"
            )
            continue
        found.append(
            f"FAIL: `{source}` -> `{target}` is a first-party normal edge layers.toml "
            "does not allow"
        )
    for source in sorted(layers.deps):
        for target, pending in sorted(layers.deps[source].items()):
            present = (source, target) in edges
            if pending and present:
                found.append(
                    f"FAIL: `{source}` -> `{target}` exists now; write it as a present edge "
                    "(drop `pending = true`)"
                )
            elif not pending and not present and source in members:
                found.append(
                    f"FAIL: `{source}` -> `{target}` is listed but the graph has no such "
                    "edge (STALE); remove it or mark it pending"
                )
    loop = cycle(layers)
    if loop:
        found.append(f"FAIL: the declared graph has a cycle: {' -> '.join(loop)}")
    return found


def forbid_unsafe_failures(crates_dir: Path) -> list[str]:
    """One line per crate whose lib.rs lacks the crate-level forbid attribute."""
    found: list[str] = []
    for name in FORBID_UNSAFE_CRATES:
        lib = crates_dir / name / "src" / "lib.rs"
        try:
            lines = lib.read_text(encoding="utf-8").splitlines()
        except OSError as exc:
            found.append(f"FAIL: crates/{name}/src/lib.rs unreadable: {exc}")
            continue
        if FORBID_UNSAFE_ATTRIBUTE not in (line.strip() for line in lines):
            found.append(f"FAIL: crates/{name}/src/lib.rs must carry `{FORBID_UNSAFE_ATTRIBUTE}` (no unsafe policy)")
    return found


def normal_dependencies(graph) -> dict[str, set[str]]:
    """member name -> the names of every package it depends on through a NORMAL
    edge, on any target, first-party and external alike."""
    return {
        graph.names[pid]: {
            graph.names[dep] for dep, kinds in graph.edges.get(pid, ()) if "normal" in kinds
        }
        for pid in graph.workspace
    }


def ring_fence_failures(layers: Layers, dependencies: dict[str, set[str]]) -> list[str]:
    """Every breach of the kernel ring-fence: a normal dependency of the root, a
    normal dependency of a ring-fenced crate its row does not list, and a listed
    external dependency the crate no longer has."""
    found: list[str] = []
    for name in sorted(dependencies.get(layers.root, set())):
        found.append(
            f"FAIL: `{layers.root}` is the root and must have zero runtime dependencies, "
            f"but depends on `{name}`"
        )
    for crate, external in sorted(layers.external.items()):
        if crate not in dependencies:
            if crate not in layers.pending_crates:
                found.append(f"FAIL: ring-fenced `{crate}` is not a workspace member")
            continue
        allowed = set(layers.deps[crate]) | external
        for name in sorted(dependencies[crate] - allowed):
            found.append(
                f"FAIL: ring-fenced `{crate}` depends on `{name}`, which its layers.toml row "
                "lists neither in `deps` nor in `external`"
            )
        for name in sorted(external - dependencies[crate]):
            found.append(
                f"FAIL: ring-fenced `{crate}` lists `{name}` in `external` but no longer "
                "depends on it (STALE); remove it"
            )
    return found


def reach(layers: Layers, start: str) -> set[str]:
    """Every crate ``start`` reaches over the declared graph, itself included."""
    seen = {start}
    stack = [start]
    while stack:
        for dep in layers.deps[stack.pop()]:
            if dep not in seen:
                seen.add(dep)
                stack.append(dep)
    return seen


def home_for(layers: Layers, callers: list[str]) -> list[str]:
    """The common dependencies of ``callers`` closest to them."""
    unknown = [name for name in callers if name not in layers.deps]
    if unknown:
        raise LayersError(f"no layers.toml row for {', '.join(unknown)}")
    common = set.intersection(*(reach(layers, name) for name in callers))
    if not common:
        return [layers.root]
    reached = {name: reach(layers, name) - {name} for name in common}
    return sorted(
        name
        for name in common
        if not any(name in reached[other] for other in common if other != name)
    )


def fixture_graph(extra_edges: tuple[tuple[str, str], ...] = (), members: tuple[str, ...] = ("r", "a", "b", "c")):
    """A resolved graph of first-party members: c -> b -> a -> r, plus ``extra_edges``."""
    edges = {("b", "a"), ("c", "b"), ("a", "r")} | set(extra_edges)

    def package(name: str) -> dict:
        return {"id": name, "name": name, "source": None, "publish": None, "targets": [{"kind": ["lib"]}]}

    return BANNED_DEPS.graph_from_metadata(
        {
            "workspace_members": list(members),
            "packages": [package(name) for name in members],
            "resolve": {
                "nodes": [
                    {
                        "id": name,
                        "deps": [
                            {"pkg": target, "dep_kinds": [{"kind": None, "target": None}]}
                            for source, target in sorted(edges)
                            if source == name and target in members
                        ],
                    }
                    for name in members
                ]
            },
        }
    )


FIXTURE_LAYERS = """
root = "r"
[[crate]]
name = "r"
deps = []
[[crate]]
name = "a"
deps = ["r"]
[[crate]]
name = "b"
deps = ["a", { name = "p", pending = true }]
[[crate]]
name = "c"
deps = ["b"]
[[crate]]
name = "p"
pending = true
deps = []
"""


def self_test() -> int:
    """Seeded positive and negative cases for every failure and for --home-for."""
    cases: list[tuple[str, bool]] = []
    layers = parse_layers(tomllib.loads(FIXTURE_LAYERS))

    def run(graph) -> list[str]:
        members, edges = resolved_edges(graph)
        return failures(layers, members, edges)

    cases.append(("the declared graph, as resolved, passes", run(fixture_graph()) == []))
    cases.append(
        (
            "an undeclared first-party edge fails",
            any("`c` -> `a`" in line for line in run(fixture_graph((("c", "a"),)))),
        )
    )
    unlisted_root = run(fixture_graph((("c", "r"),)))
    cases.append(
        (
            "an unlisted edge to the root is asked to be listed, not refused as a layering breach",
            len(unlisted_root) == 1 and "always allowed" in unlisted_root[0],
        )
    )
    cases.append(
        (
            "a listed edge the graph lost is STALE",
            any("STALE" in line for line in run(_without(("c", "b")))),
        )
    )
    cases.append(
        (
            "a member with no row fails",
            any("`x` has no" in line for line in run(fixture_graph(members=("r", "a", "b", "c", "x")))),
        )
    )
    pending_present = run(fixture_graph((("b", "p"),), members=("r", "a", "b", "c", "p")))
    cases.append(
        (
            "a pending crate and edge that now exist must be flipped",
            any("drop `pending = true` from its row" in line for line in pending_present)
            and any("`b` -> `p` exists now" in line for line in pending_present),
        )
    )
    cyclic = parse_layers(tomllib.loads(FIXTURE_LAYERS.replace('deps = ["r"]', 'deps = ["r", "c"]')))
    cases.append(("a declared cycle fails", cycle(cyclic) is not None))
    cases.append(("the fixture has no cycle", cycle(layers) is None))
    cases.append(("home-for a caller and its dependency is the dependency", home_for(layers, ["c", "a"]) == ["a"]))
    cases.append(("home-for two chained callers is the lower one", home_for(layers, ["c", "b"]) == ["b"]))
    cases.append(("home-for a caller alone is itself", home_for(layers, ["b"]) == ["b"]))
    cases.append(("home-for crates that share nothing else is the root", home_for(layers, ["p", "a"]) == ["r"]))
    fenced = parse_layers(
        tomllib.loads(
            FIXTURE_LAYERS.replace('name = "a"\ndeps = ["r"]', 'name = "a"\ndeps = ["r"]\nexternal = ["ext"]')
        )
    )
    held = {"r": set(), "a": {"r", "ext"}, "b": {"a"}, "c": {"b"}}
    cases.append(("a ring-fenced crate whose every dependency is listed passes", ring_fence_failures(fenced, held) == []))
    cases.append(
        (
            "a ring-fenced crate with an unlisted external dependency fails",
            any("depends on `other`" in line for line in ring_fence_failures(fenced, {**held, "a": {"r", "ext", "other"}})),
        )
    )
    cases.append(
        (
            "an unfenced crate may take an external dependency",
            ring_fence_failures(fenced, {**held, "b": {"a", "other"}}) == [],
        )
    )
    cases.append(
        (
            "a listed external the ring-fenced crate dropped is STALE",
            any("STALE" in line for line in ring_fence_failures(fenced, {**held, "a": {"r"}})),
        )
    )
    cases.append(
        (
            "a root with any runtime dependency fails the ring-fence",
            any("zero runtime dependencies" in line for line in ring_fence_failures(fenced, {**held, "r": {"ext"}})),
        )
    )
    with_dev_and_build = fixture_graph()
    with_dev_and_build.edges["a"].append(("c", frozenset({"dev"})))
    with_dev_and_build.edges["b"].append(("r", frozenset({"build"})))
    with_dev_and_build.edges["c"].append(("a", frozenset({"normal", "dev"})))
    cases.append(
        (
            "normal edges are counted, dev-only and build-only edges are not",
            normal_dependencies(with_dev_and_build) == {"r": set(), "a": {"r"}, "b": {"a"}, "c": {"a", "b"}},
        )
    )
    for bad, why in [
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = []\nexternal = []\n', "an `external` on the root"),
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = []\n[[crate]]\nname = "a"\ndeps = []\nexternal = ["x", "x"]\n', "a doubled external"),
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = ["a"]\n[[crate]]\nname = "a"\ndeps = []\n', "a root with dependencies"),
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = []\n[[crate]]\nname = "a"\ndeps = ["z"]\n', "a dep with no row"),
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = []\n[[crate]]\nname = "a"\ndeps = [{ name = "r", pending = false }]\n', "a pending = false entry"),
        ('root = "r"\n[[crate]]\nname = "r"\ndeps = []\nlayer = 1\n', "an unknown key"),
    ]:
        try:
            parse_layers(tomllib.loads(bad))
            cases.append((f"{why} is refused", False))
        except LayersError:
            cases.append((f"{why} is refused", True))
    with tempfile.TemporaryDirectory() as scratch:
        crates = Path(scratch)
        for name in FORBID_UNSAFE_CRATES:
            (crates / name / "src").mkdir(parents=True)
            (crates / name / "src" / "lib.rs").write_text(f"{FORBID_UNSAFE_ATTRIBUTE}\n", encoding="utf-8")
        cases.append(("every forbid-unsafe crate carrying the attribute passes", forbid_unsafe_failures(crates) == []))
        (crates / "gts" / "src" / "lib.rs").write_text("// #![forbid(unsafe_code)]\n", encoding="utf-8")
        cases.append(("a crate whose forbid attribute is only commented out is reported", len(forbid_unsafe_failures(crates)) == 1))
        (crates / "gts" / "src" / "lib.rs").write_text(f"{FORBID_UNSAFE_ATTRIBUTE}\n", encoding="utf-8")
        (crates / "slice" / "src" / "lib.rs").unlink()
        cases.append(("a crate with no lib.rs is reported", len(forbid_unsafe_failures(crates)) == 1))
    mini_layers = parse_layers(
        tomllib.loads(
            'root = "r"\n[[crate]]\nname = "r"\ndeps = []\n[[crate]]\nname = "a"\ndeps = ["r"]\n'
            '[[crate]]\nname = "b"\ndeps = []\n'
        )
    )
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["r", "a", "b"]\n', encoding="utf-8")
        for crate, deps in (("r", ""), ("a", 'r = { workspace = true }\nbogus = "1"\n')):
            crate_dir = root / crate
            crate_dir.mkdir()
            (crate_dir / "Cargo.toml").write_text(
                f'[package]\nname = "{crate}"\n[dependencies]\n{deps}', encoding="utf-8"
            )
        (root / "b").mkdir()
        (root / "b" / "Cargo.toml").write_text('[package]\nname = "b"\n', encoding="utf-8")
        cases.append(("the fast pre-check passes an agreeing mini-tree", fast_failures(mini_layers, root) == []))
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dependencies]\nr = { workspace = true }\nb = { workspace = true }\n',
            encoding="utf-8",
        )
        wrong = fast_failures(mini_layers, root)
        cases.append(
            ("a wrong first-party edge fails the fast pre-check (exit-1 class)", any("`a` -> `b`" in line for line in wrong))
        )
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dependencies]\nb = { workspace = true }\n', encoding="utf-8"
        )
        wrong = fast_failures(mini_layers, root)
        cases.append(
            (
                "a missing first-party edge is reported both ways by the fast pre-check",
                any("`a` -> `r` is listed in layers.toml but not declared" in line for line in wrong)
                and any("`a` -> `b` is declared" in line for line in wrong),
            )
        )
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dependencies]\nrs = { workspace = true }\n', encoding="utf-8"
        )
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["r", "a", "b"]\n[workspace.dependencies]\nrs = { package = "r" }\n',
            encoding="utf-8",
        )
        cases.append(("an aliased workspace dependency resolves to its package", fast_failures(mini_layers, root) == []))
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dependencies]\n'
            "[target.'cfg(unix)'.dependencies]\nr = { workspace = true }\n",
            encoding="utf-8",
        )
        cases.append(("a target-table first-party edge enters the static graph", ("a", "r") in static_graph(root)[1]))
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dev-dependencies]\nb = { workspace = true }\n',
            encoding="utf-8",
        )
        cases.append(
            ("a dev-only first-party edge stays out of the static graph", ("a", "b") not in static_graph(root)[1])
        )
        (root / "a" / "Cargo.toml").write_text(
            '[package]\nname = "a"\n[dependencies]\nrr = { workspace = true }\n',
            encoding="utf-8",
        )
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["r", "a", "b"]\n[workspace.dependencies]\nrr = { package = "r" }\n',
            encoding="utf-8",
        )
        cases.append(("a renamed workspace dependency resolves to its package", ("a", "r") in static_graph(root)[1]))
    real_edges = static_graph(REPO_ROOT)[1]
    cases.append(("the static resolver finds a known real edge", ("purrdf-core", "purrdf-hash") in real_edges))
    cases.append(("the static resolver invents no reversed edge", ("purrdf-hash", "purrdf-core") not in real_edges))
    failed = 0
    for name, held in cases:
        print(f"{'PASS' if held else 'FAIL'}: {name}")
        failed += not held
    if failed:
        print(f"check-layers self-test: {failed} case(s) failed")
        return 1
    print("OK: check-layers self-test")
    return 0


def _without(edge: tuple[str, str]):
    """The fixture graph with one of its own edges removed."""
    graph = fixture_graph()
    source, target = edge
    graph.edges[source] = [(dep, kinds) for dep, kinds in graph.edges[source] if dep != target]
    return graph


def load_layers() -> Layers:
    with LAYERS_PATH.open("rb") as handle:
        return parse_layers(tomllib.load(handle))


def workspace_members(repo_root: Path) -> tuple[dict[str, Path], dict[str, str]]:
    """Every workspace member's package name -> its manifest, read from the root
    manifest's ``[workspace]`` table (globs expanded); plus the
    ``[workspace.dependencies]`` alias table (key -> package name) so a member's
    ``x = { workspace = true }`` entry resolves to the package it really names.
    A member whose manifest is unreadable or has no name is skipped."""
    try:
        document = tomllib.loads((repo_root / "Cargo.toml").read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return {}, {}
    table = document.get("workspace", {})
    aliases = {
        key: entry["package"]
        for key, entry in table.get("dependencies", {}).items()
        if isinstance(entry, dict) and isinstance(entry.get("package"), str)
    }
    names: dict[str, Path] = {}
    for pattern in table.get("members", []):
        if not isinstance(pattern, str):
            continue
        candidates = [repo_root / pattern / "Cargo.toml"] if not any(c in pattern for c in "*?[") else sorted(repo_root.glob(pattern))
        for found in candidates:
            manifest = found if found.name == "Cargo.toml" else found / "Cargo.toml"
            try:
                name = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["name"]
            except (OSError, tomllib.TOMLDecodeError, KeyError, TypeError):
                continue
            names[name] = manifest
    return names, aliases


def resolved_first_party(
    section: dict, manifest: Path, names: dict[str, Path], aliases: dict[str, str]
) -> set[str]:
    """The first-party workspace packages one dependency section declares: a
    ``workspace = true`` entry resolves through the alias table (``x = { package
    = "…" }`` resolves to the package it names), a ``path`` entry resolving to a
    member manifest resolves to that member's name, a plain string resolves
    through the aliases too; anything else keeps its key and is kept only if it
    names a member."""
    found: set[str] = set()
    for key, value in section.items():
        if isinstance(value, dict):
            if "workspace" in value:
                name = aliases.get(key, key)
            else:
                name = value.get("package", key)
                if "path" in value:
                    target = (manifest.parent / value["path"] / "Cargo.toml").resolve()
                    name = next((n for n, m in names.items() if m.resolve() == target), name)
        else:
            name = aliases.get(key, key)
        if name in names:
            found.add(name)
    return found


def manifest_first_party_deps(
    manifest: Path, names: dict[str, Path], aliases: dict[str, str]
) -> set[str]:
    """The first-party workspace packages a member's top-level ``[dependencies]``
    section declares. NECESSARY-NOT-SUFFICIENT (the ``--fast`` pre-check's
    resolver): reads only that section — ``[dev-dependencies]``,
    ``[build-dependencies]`` and ``[target.*]`` tables are not layering or are
    not read; features and workspace resolution are the full gate's job. The
    DEFAULT mode uses ``static_graph`` below, which reads the normal sections
    on every target."""
    try:
        document = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return set()
    section = document.get("dependencies")
    if not isinstance(section, dict):
        return set()
    return resolved_first_party(section, manifest, names, aliases)


def normal_sections(document: dict) -> list[dict]:
    """Every table of a parsed manifest that declares NORMAL dependencies: the
    top-level ``[dependencies]`` plus each ``[target.<spec>.dependencies]``.
    ``[dev-dependencies]`` and ``[build-dependencies]`` never enter the edge
    set — layering is normal-only."""
    sections: list[dict] = []
    top = document.get("dependencies")
    if isinstance(top, dict):
        sections.append(top)
    for spec, table in document.get("target", {}).items():
        if isinstance(table, dict):
            section = table.get("dependencies")
            if isinstance(section, dict):
                sections.append(section)
    return sections


def static_graph(repo_root: Path) -> tuple[dict[str, Path], set[tuple[str, str]]]:
    """(member name -> manifest, first-party NORMAL edges) resolved statically
    from the manifests — no cargo subprocess, no Cargo.lock. Each member's
    ``[dependencies]`` and ``[target.*.dependencies]`` tables are resolved
    against the member set (dev and build dependencies excluded, layering being
    normal-only), and the result is intersected with the members — identical
    semantics to ``resolved_edges`` over cargo's graph. ``failures()`` diffs
    this edge set against the layers.toml rows exactly as it does the cargo
    one; ``--via-cargo`` asserts the two resolvers agree."""
    names, aliases = workspace_members(repo_root)
    edges: set[tuple[str, str]] = set()
    for source, manifest in names.items():
        try:
            document = tomllib.loads(manifest.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError):
            continue
        for section in normal_sections(document):
            for target in resolved_first_party(section, manifest, names, aliases):
                edges.add((source, target))
    return names, edges


def fast_failures(layers: Layers, repo_root: Path) -> list[str]:
    """Every exact disagreement between each member's declared first-party
    ``[dependencies]`` and its row's present edges — the fast pre-check."""
    names, aliases = workspace_members(repo_root)
    found: list[str] = []
    for name in sorted(set(names) - set(layers.deps)):
        found.append(
            f"FAIL: crate `{name}` has no [[crate]] row in layers.toml (fast pre-check; "
            "the full gate gives the authoritative verdict)"
        )
    for name in sorted(set(layers.deps) - set(names) - layers.pending_crates):
        found.append(f"FAIL: layers.toml row `{name}` names no workspace member (STALE; fast pre-check)")
    for source in sorted(set(names) & set(layers.deps) - layers.pending_crates):
        row = layers.deps[source]
        present = {dep for dep, pending in row.items() if not pending}
        declared = manifest_first_party_deps(names[source], names, aliases)
        for target in sorted(declared - present):
            if target in row and row[target]:
                found.append(
                    f"FAIL: `{source}` -> `{target}` is declared in Cargo.toml but its row "
                    "still marks it pending; flip it to present"
                )
            else:
                found.append(
                    f"FAIL: `{source}` -> `{target}` is declared in Cargo.toml but layers.toml "
                    "does not list it as a present edge (fast pre-check)"
                )
        for target in sorted(present - declared):
            found.append(
                f"FAIL: `{source}` -> `{target}` is listed in layers.toml but not declared in "
                "the crate's [dependencies] (STALE or feature/target-resolved; fast pre-check, "
                "the full gate decides)"
            )
    loop = cycle(layers)
    if loop:
        found.append(f"FAIL: the declared graph has a cycle: {' -> '.join(loop)}")
    return found


def main() -> int:
    arguments = sys.argv[1:]
    if arguments == ["--self-test"]:
        return self_test()
    try:
        layers = load_layers()
    except (OSError, tomllib.TOMLDecodeError, LayersError) as exc:
        print(f"FAIL: layers.toml: {exc}")
        return 1
    if arguments == ["--ring-fence"]:
        try:
            graph = BANNED_DEPS.graph_from_metadata(BANNED_DEPS.cargo_metadata(REPO_ROOT / "Cargo.toml"))
        except BANNED_DEPS.MetadataError as exc:
            print(f"FAIL: {exc}")
            return 1
        found = ring_fence_failures(layers, normal_dependencies(graph))
        if found:
            for line in found:
                print(line)
            return 1
        fenced = ", ".join(sorted(layers.external))
        print(
            f"OK: `{layers.root}` has zero runtime dependencies; every runtime dependency of "
            f"{fenced} is listed in its layers.toml row"
        )
        return 0
    if arguments == ["--fast"]:
        started = perf_counter()
        found = fast_failures(layers, REPO_ROOT)
        elapsed_ms = (perf_counter() - started) * 1000.0
        if found:
            for line in found:
                print(line)
            return 1
        print(
            f"OK (fast pre-check): every workspace member's declared [dependencies] match "
            f"layers.toml ({sum(name not in layers.pending_crates for name in layers.deps)} non-pending rows); necessary-not-sufficient, "
            f"run the full gate for the authoritative verdict; {elapsed_ms:.1f} ms"
        )
        return 0
    if arguments[:1] == ["--home-for"]:
        callers = arguments[1:]
        if not callers:
            print("usage: check-layers.py --home-for CRATE [CRATE ...]", file=sys.stderr)
            return 2
        try:
            homes = home_for(layers, callers)
        except LayersError as exc:
            print(f"FAIL: {exc}", file=sys.stderr)
            return 1
        for name in homes:
            print(name)
        return 0
    if arguments == ["--via-cargo"]:
        started = perf_counter()
        s_names, s_edges = static_graph(REPO_ROOT)
        try:
            graph = BANNED_DEPS.graph_from_metadata(BANNED_DEPS.cargo_metadata(REPO_ROOT / "Cargo.toml"))
        except BANNED_DEPS.MetadataError as exc:
            print(f"FAIL: {exc}")
            return 1
        c_members, c_edges = resolved_edges(graph)
        elapsed_ms = (perf_counter() - started) * 1000.0
        member_diff = sorted(set(s_names) ^ c_members)
        edge_diff = sorted(s_edges ^ c_edges)
        if member_diff or edge_diff:
            print("FAIL: the static and cargo resolutions of the workspace graph disagree:")
            for name in member_diff:
                side = "static only" if name in s_names else "cargo only"
                print(f"FAIL: member `{name}` ({side})")
            for source, target in edge_diff:
                side = "static only" if (source, target) in s_edges else "cargo only"
                print(f"FAIL: edge `{source}` -> `{target}` ({side})")
            return 1
        found = failures(layers, c_members, c_edges) + forbid_unsafe_failures(REPO_ROOT / "crates")
        if found:
            for line in found:
                print(line)
            return 1
        print(
            f"OK: static {len(s_edges)} edges / {len(s_names)} members; "
            f"cargo {len(c_edges)} edges / {len(c_members)} members — IDENTICAL; "
            f"layers.toml agrees; {elapsed_ms:.0f} ms"
        )
        return 0
    if arguments:
        print(
            "usage: check-layers.py [--self-test | --fast | --via-cargo | --ring-fence"
            " | --home-for CRATE [CRATE ...]]",
            file=sys.stderr,
        )
        return 2
    started = perf_counter()
    names, edges = static_graph(REPO_ROOT)
    elapsed_ms = (perf_counter() - started) * 1000.0
    found = failures(layers, set(names), edges) + forbid_unsafe_failures(REPO_ROOT / "crates")
    if found:
        for line in found:
            print(line)
        return 1
    pending = sum(pending for deps in layers.deps.values() for pending in deps.values())
    print(
        f"OK: {len(edges)} first-party normal edges across {len(names)} members match "
        f"layers.toml (static manifest resolution, no cargo; {pending} planned edges, "
        f"{len(layers.pending_crates)} planned crates); "
        f"{len(FORBID_UNSAFE_CRATES)} crates forbid unsafe_code; {elapsed_ms:.0f} ms"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
