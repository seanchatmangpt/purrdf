# OPT-69: Python binding silently discards the provenance exit gate

**Status:** OPEN (ticketed 2026-10-27, from the .pyi-truth Explore lane)
**Priority:** medium — correctness of the provenance report surface, not data corruption
**Surface:** `bindings/python/src/py_store/results.rs`

## Finding

`SerializeOutcome::provenance_dropped` is `#[must_use = "provenance_dropped
is an exit gate; discarding it discards the report"]`
(`crates/sparql-results/src/lib.rs:265`) and the book promises "the drop is
never silent" (`docs/book/src/sparql/results.md:49`). The Python binding
(`serialize_sparql_solutions`, results.rs ~93-121 and its boolean twin)
keeps only `outcome.bytes` and returns a bare `PyBytes`, silently
discarding the flag. Canon-arrow **always** drops provenance
(lib.rs:330-333), so a Python caller passing `provenance_namespace=` to a
canon-arrow serialize loses the report with zero indication — the exit gate
is unhonorable from Python.

## Mechanical replacement

1. Change the Python return to a small result object (or tuple)
   `(bytes, provenance_dropped: bool)` — or add a keyword variant
   `serialize_sparql_solutions_ex` if the flat return is a breaking change
   for the 3.0.1 lockstep (decide: it is PyPI-lockstep, so a breaking
   change is allowed only at a minor bump per docs/RELEASE.md).
2. Mirror in `__init__.pyi` stub.
3. Test: canon-arrow serialize with a namespace → flag is `True`; csv →
   `False`.
4. Update `docs/book/src/sparql/results.md` Python section if it shows the
   bare-bytes return.

## Verification invariant

After fix: no Python path exists where provenance can be dropped without
the caller receiving the flag. The `#[must_use]` at the Rust boundary must
have a Python-visible counterpart.
