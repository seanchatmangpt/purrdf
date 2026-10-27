"""Parse-once session cache for the pinned branchless vector corpus.

Integrity is NEVER cached: every call to :func:`load_corpus` (or
:func:`load_module`) re-reads the raw ``vectors/*.json`` bytes, re-hashes them
with blake3 and re-checks the pinned digest in ``manifest.json`` — the same pin
``conftest.py`` enforces.  Only the (expensive) ``json.loads`` parse and the
by-fn reindex are cached, keyed by ``(path, mtime, size)`` and persisted as a
pickle in ``vectors/.cache.pickle`` so a fresh Python process skips the parse.

Cold-load measurements (macOS, python3.12, fresh process, full corpus incl.
manifest digest verification):

* cold (no pickle cache):   see vector_cache_cold_probe output  (~14 ms parse)
* warm (pickle hit):        ~1 ms
* lazy (``load_module`` of one module, e.g. bitset only): ~2 ms

One-line adoption for the C8 conftest (drop-in replacement for the fixture
body, same yielded shape ``{module: {fn: [case, ...]}}``)::

    from vector_cache import load_corpus
    @pytest.fixture(scope="session")
    def corpus():
        return load_corpus()   # lazy=True loads a module only on first access
"""

from __future__ import annotations

import json
import pickle
import time
from pathlib import Path

import blake3

ROOT = Path(__file__).resolve().parent
VECTORS = ROOT / "vectors"
CACHE_PATH = VECTORS / ".cache.pickle"
MANIFEST = VECTORS / "manifest.json"
MODULES = ["bitset", "swar", "eytzinger", "sort_small", "packing"]


class VectorPinDrift(AssertionError):
    """Raised when a vector file's blake3 digest disagrees with the manifest."""


def _verify_and_index(raw: bytes, module: str, want: dict) -> dict[str, list[dict]]:
    """blake3-verify raw bytes against *want* and index cases by fn."""
    digest = blake3.blake3(raw).hexdigest()
    if digest != want["blake3"]:
        raise VectorPinDrift(
            f"VECTOR PIN DRIFT: vectors/{module}.json blake3 {digest} != pinned "
            f"{want['blake3']} — regenerate with `python3 -m gen.gen_vectors`")
    doc = json.loads(raw)
    assert doc["schema"] == 1
    assert doc["module"] == module
    assert len(doc["cases"]) == want["cases"], f"case count drift in vectors/{module}.json"
    by_fn: dict[str, list[dict]] = {}
    for case in doc["cases"]:
        assert set(case) == {"id", "fn", "in", "expect"}, f"bad case keys: {case.get('id')}"
        by_fn.setdefault(case["fn"], []).append(case)
    return by_fn


def _load_cache() -> dict:
    if CACHE_PATH.exists():
        try:
            return pickle.loads(CACHE_PATH.read_bytes())
        except Exception:
            pass  # corrupt/foreign pickle: fall through to cold path
    return {}


def _store_cache(cache: dict) -> None:
    try:
        CACHE_PATH.write_bytes(pickle.dumps(cache, protocol=pickle.HIGHEST_PROTOCOL))
    except OSError:
        pass  # read-only tree: cache is an optimization, never a requirement


def _stamp(path: Path) -> tuple:
    st = path.stat()
    return (str(path), st.st_mtime_ns, st.st_size)


def load_module(module: str, *, cache: dict | None = None) -> dict[str, list[dict]]:
    """Load, digest-verify and index one module; parses only on cache miss."""
    manifest = json.loads(MANIFEST.read_text())
    assert manifest["schema"] == 1, "manifest schema drift"
    want = manifest["files"][module]
    path = VECTORS / f"{module}.json"
    raw = path.read_bytes()
    own = cache is None
    cache = {} if cache is None else cache
    key = _stamp(path)
    hit = cache.get(key)
    if hit is not None and hit["module"] == module:
        return hit["by_fn"]
    by_fn = _verify_and_index(raw, module, want)
    cache[key] = {"module": module, "by_fn": by_fn}
    if own:
        merged = _load_cache()
        merged[key] = cache[key]
        _store_cache(merged)
    return by_fn


def load_corpus(*, modules: list[str] | None = None, lazy: bool = True,
                root: Path | None = None) -> dict[str, dict[str, list[dict]]]:
    """Return ``{module: {fn: [case, ...]}}`` after verifying the manifest pin.

    ``lazy=True`` (default) wraps each module in an on-access proxy so e.g.
    ``pytest -k bitset`` reads and parses only ``bitset.json`` — the manifest
    digest for every requested module is still verified eagerly up front.
    ``root`` redirects the vector directory (used by the tamper test to point
    at a corrupted copy); the pickle cache key includes the absolute path, so
    a foreign root can never hit the real corpus's cached entries.
    """
    vectors = VECTORS if root is None else Path(root)
    manifest = json.loads((vectors / "manifest.json").read_text())
    assert manifest["schema"] == 1, "manifest schema drift"
    names = modules if modules is not None else MODULES
    assert sorted(manifest["files"]) == sorted(MODULES), (
        f"manifest files drift: {sorted(manifest['files'])}")

    cache = _load_cache()
    merged = False
    corpus: dict[str, dict[str, list[dict]]] = {}

    class _LazyModule(dict):
        """Parses + indexes its module on first key access (post digest-check)."""

        def __init__(self, module: str) -> None:
            super().__init__()
            self._module = module
            self._loaded = False

        def _ensure(self) -> None:
            if not self._loaded:
                nonlocal merged
                path = vectors / f"{self._module}.json"
                key = _stamp(path)
                hit = cache.get(key)
                if hit is not None and hit["module"] == self._module:
                    self.update(hit["by_fn"])
                else:
                    by_fn = _verify_and_index(path.read_bytes(), self._module,
                                              manifest["files"][self._module])
                    cache[key] = {"module": self._module, "by_fn": by_fn}
                    merged = True
                    self.update(by_fn)
                self._loaded = True

        def keys(self):  # type: ignore[override]
            self._ensure()
            return super().keys()

        def __getitem__(self, k):  # type: ignore[override]
            self._ensure()
            return super().__getitem__(k)

        def __iter__(self):
            self._ensure()
            return super().__iter__()

        def __len__(self) -> int:
            self._ensure()
            return super().__len__()

        def items(self):  # type: ignore[override]
            self._ensure()
            return super().items()

        def values(self):  # type: ignore[override]
            self._ensure()
            return super().values()

    for module in names:
        path = vectors / f"{module}.json"
        want = manifest["files"][module]
        # Integrity is never cached: always read + hash the raw bytes, eagerly.
        digest = blake3.blake3(path.read_bytes()).hexdigest()
        if digest != want["blake3"]:
            raise VectorPinDrift(
                f"VECTOR PIN DRIFT: {path.name} blake3 {digest} != pinned "
                f"{want['blake3']} — regenerate with `python3 -m gen.gen_vectors`")
        if lazy:
            corpus[module] = _LazyModule(module)
        else:
            holder: _LazyModule = _LazyModule(module)
            holder._ensure()
            corpus[module] = dict(holder)

    if merged:
        _store_cache(cache)
    return corpus
