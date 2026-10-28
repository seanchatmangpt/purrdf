# OPT-77: wave-8 per-crate audit findings (mechanical/deferred set)

**Status:** OPEN (filed 2026-10-27, wave-8). Landed fixes from this wave
are in the commit history (schema_import overflow guards, Scalar Debug,
identity count, retrieval obligation count, gitignore negation,
seminaive comment, guard allowlist); the items below need their own
passes.

## slice (silent-success class)

- `crates/slice/src/cache.rs:326-367` — `dependency_closure` silently
  drops unknown seeds; `product_unit_key` over a bogus seed returns
  `Ok` with an empty member set (vacuous Merkle key) where
  `source_unit_key` correctly raises `UnknownCacheSlice`. Fix: same
  refusal.
- `crates/slice/src/cache.rs:435-442` — phase-leaf map keyed by logical
  path alone; duplicate logical paths silently overwrite. Fix: assert
  or typed refusal on collision.

## mime / toml / glob (diagnostic-surface class)

- `crates/mime/src/error.rs:47-48` vs `transfer.rs:53-77` — `at` is 0
  for Unknown/Ambiguous/base64 failures; the true xsd offset is
  discarded. Fix: thread `parse_base64`'s offset through.
- `crates/mime/src/decode.rs:134-138` — `document` never IRI-validated
  (TOML validates at `toml/src/codec.rs:315`); prefix-matching on an
  odd document produces misleading Metadata failures.
- `crates/toml/src/parse.rs:340-341` — raw control char reported as
  "an escaped string control character" (inverted).
- `crates/toml/src/parse.rs:333-335` — newline-present error says
  "single-line string without a newline" (inverted); should name the
  missing closing quote.
- `crates/toml/src/parse.rs:211-213` vs `:365-367` — key depth is
  `>=` (exclusive) while node depth is `>` (inclusive) against the same
  `max_depth`; reconcile semantics.
- `crates/glob/src/lib.rs:55` — "offsets always refer to..." needs a
  clarifier for `EmptyClass`/`UnclosedClass` positions.

## text / retrieval (doc class)

- `crates/text/benches/search.rs:391-416` — bm25f group has no
  throughput declaration (unitless rows).
- `crates/retrieval/benches/fusion_frontier_alloc.rs:139-141` and
  `:404-406` — stale grid-cost claims (largest drained total is 16k,
  not millions; the "never reported" sentence contradicts main).
- `crates/retrieval/src/lib.rs:454` — `pub mod fixture` ships test
  fixtures in the production surface (doc-hidden; consider a dev-home).

## shapes (sibling to the landed overflow guards)

- `schema_import.rs:2711` + consumers `:1398-1414` — a wrapped
  MinLength(0) previously propagated as "no minimum" with no ledger
  entry; the landed checked_add guards close the wrap, but the
  lossless-import contract deserves a test for the u64::MAX
  round-trip (emit → re-import) pinning the refusal.

## hnsw

- `HnswError::EmptyGraph` (error.rs:182) is dead — never constructed
  outside its own display test; remove the variant or document the
  invariant that makes it unreachable.
- `PURRDF_HNSW_BENCH_SCALES` caller order is silently ignored (output
  follows LADDER order) — one README sentence.

## helper-census

- helpers-ledger.toml census paragraph omits the MIN_TOKENS=20 floor.

## Verification invariants

Each item closes with the named refusal/test landing; the toml error
texts close with a round-trip test asserting the corrected `expected`
string.
