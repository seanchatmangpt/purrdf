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

## gts (wave-8 lane, appended)

- README.md:84-88 vs manifests vs GTS-CONFORMANCE.md:242 — three-way
  disagreement on the vector manifests' `generated_by` (README says a
  never-built local generator; manifests say
  scripts/check_vector_manifest.py; the script exists nowhere). One
  truth needed.
- Fuzz gap: the fuzz target exercises only reader::read;
  read_with_options + the whole read_to_sink streaming machinery have
  zero fuzz coverage.
- cose.rs:171-175 — Encrypt0 kid decoded as UTF-8-only while Sign1
  treats kid as opaque bytes (RFC 9052 says opaque); same-crate
  contract split.
- reader.rs:2296-2310 — dead branch with a misleading diagnostic
  (copy-paste of the first-item branch).
- cose.rs:89-113 — expect on AES-GCM encrypt: a panic on the
  production encrypt0 path if the crate ever errors.
- reader.rs:1742-1761/2256-2274 — expected_head silently unevaluated
  for multi-segment files with allow_segments=false (docs don't state
  the interaction).

## conformance crates (appended)

- owl2_rl.rs:458-460 + lib.rs:24 — stale comment (retired test name)
  and 261-vs-262 case count: FIXED in-tree.
- datalog: evidence.rs absense/found_path/aggregate are dead pub API;
  ClauseTerm::surface Panics doc: FIXED in-tree; bench header counts
  annotated.
- envelope-probe main.rs:74-78 — a negative allocator peak is clamped
  to 0 and PASSES the roundtrip acceptance criterion (counter defect
  converts to quiet pass); --groups override is judged against the
  canonical-scale ceiling; --groups 0 admitted.
- LUBM_PROFILE.md:8 — the documented draw order omits two draws (head
  index, ResearchGroup count); a faithful reimplementation from prose
  yields a different stream.
- geo-kernel: hilbert_path parity test covers levels 0..=16 while the
  production caller uses MAX_LEVEL=30 (coverage gap at the clamp
  boundary; branchless-side tests do cover 30).

## json / columnar (appended)

- json decode.rs:60-67/:199-214 — multi-fault refusal selection is
  insertion-order dependent (FastMap iteration); error text
  deterministic per build but not canonical. Fix: sort candidate
  errors before selection.
- json parse.rs:119-123 — dead refusal arms (StringBytes /
  DuplicateMember unreachable under DEFAULT limits); the coupling
  comment pins it.
- columnar reader.rs:701-732 — graph-refusal context hardcoded to
  "graph reference" (no table/column, unlike sibling checks);
  reader.rs:547-551 DigestOrder refusal loses the `row` field.
- columnar writer.rs:504-559 — duplicate quad streams silently
  collapse (set semantics; no ledger entry).
- cli: help_cli.rs:32-48 pinned subcommand list omits 6 shipped verbs
  (GlobRdf/GlobSet/Glob/Geo/Lint/Verify) — the drift guard itself has
  drifted; sink.rs:259-286 one doc block bound to the wrong fn;
  pack_input.rs:43-48 `tier1_sealed_memfd` label measures Tier-2 on
  non-Linux; error.rs:177 DownstreamClosed "never printed" comment
  contradicts run()'s unconditional eprintln.

## lex / xsd / validate / stack (appended)

- xsd README omits the exact tower entirely (fixed in-tree: coverage
  line added); benches cover no Rat/geometry face; Rat has no in-crate
  unit tests (delegated downstream); exponent-cap prose duplicated in
  certified.rs:446-452 and geometry.rs:354-363 (drift trap).
- stack Cargo.toml comment still claims the parser refuses against
  stack (dev-dep only) — anchor miss, fix pending.
- stack widen (lib.rs ~368-378): bytes==0 returns success before the
  EXHAUSTED check — doc precedence mismatch (cosmetic).
- geo-kernel + envelope-probe + LUBM items: see prior append.

## sql (wave-9 retry lane)

- compiler.rs:2004 — memo lookup happens AFTER SHACL validation and
  canonicalization: the "memo hit never re-runs" doc claim is false;
  every call reparses/canonicalizes.
- compiler.rs:1991 — the same Turtle document is parsed 3x per compile
  (validate_ast_shacl, DatasetReader::from_turtle, canonical_graph_bytes).
- compiler.rs:2028 — memo overflow does memo.clear() (full 256-entry
  thrash on the 257th distinct query, not LRU).
- ast.rs:5 — ast re-exports its core statement type FROM serializer.rs
  (layering inversion / drift risk).
- ~197 hardcoded graphlaw.org/sql# literals across compiler.rs + ast.rs
  (no vocab module; extends OPT-72's class into sql's own namespace).

## wave-9 additions

- deflate kernels.rs <16-byte comment: FIXED in-tree (byte-loop case
  named); bench sample-gate triplication noted, harmless.
- jsonschema: the 2019-09 hyper-schema metaschema fixture is absent
  (6 meta + root vs 2020-12's 9) — a 2019-09 schema pointing at it
  fails MissingMetaschema; `MatchLimits` "shared budget" wording is
  misleading (fresh per call).
- sql: see prior append (memo-after-validation, triple reparse,
  clear-on-overflow, ast/serializer inversion, graphlaw literals).

## wave-9 final additions

- rdf-core: visit_blank_identities covers only the base leg (delta
  values + declared graphs still full-walked, mutable.rs:619-641) -
  OPT-50 open half is larger than ticketed; test-only diagnostic-code
  drift x-purrdf-private-extension vs -ext (target.rs:1420/1432);
  evidence.rs types are cross-crate-test-only consumers.
- cdt: Cargo.toml [lib] comment claims iri+xsd are the ONLY deps but
  five exist (stale); canonical_lexical_len + the per-part
  conformance fns are dead pub surface; the injectivity caveat is
  doc-only, not a typed invariant.
- sparql-results: README deps line FIXED in-tree; frame_total expect
  + double recomputation (:139, :420, :426) -> OPT-71 item; readers
  carry no byte/line positions in any error (both json + xml).
- testkit stats.rs:169 - "paired resamples" is independent resampling
  from one stream: FIXED in-tree (wording).
- rdf: media_type.rs:100/:376 "(none today)" parenthetical is false
  (Gts carries loss_codec_name Some("gts")); viz.rs:2 header omits
  the registered-mark form; projections::loss is private with no
  re-export (undocumented intentional?).
- book: performance.md total updated to 1,693 in-tree; projections.md
  example writes /tmp/terms.tar (cosmetic).
- shacl corpora: PASS (73/64/686 pins all verify).
- SATURATION VERDICT (wave-9 lane): TODO/FIXME density ~0 across a
  133k-LOC sample; dead_code/deprecated ~0.008/kLOC. The marker-class
  audit surface is saturated; further waves should be semantic/logic
  reviews, not another 20-lane marker sweep.

- benches final lane: BENCHMARKS.md inventory omits 11 bench files
  (cli x2, json x3, jsonschema, deflate, ed25519, markdown, slice,
  gts/rfc3339); gts/authoring.rs:265 declares Throughput::Elements(1)
  on a multi-frame loop (meaningless rate); sql has no benches at all.
  All sample_size values >=10 (compliant).

## wave-9 last lanes (rdf-wasm, rdf-capi)

- rdf-capi rowcursor: post-exhaustion `_term` reads serve the stale
  last row instead of InvalidArgument (the "no row is current" guard
  covers only the pre-first-_next state).
- rdf-capi serialize.rs: the C-callback panic-abort path (extern "C"
  ABI; ffi_try cannot intercept) is undocumented; purrdf.h uses
  Rust-qualified names (PurrdfStatus::*) in C-facing doc comments
  instead of PURRDF_STATUS_*.
- rdf-wasm: cloudflare.d.ts documents none of the async job surface
  (scoped or gap — undecided); OPT-71 residue confirmed live.
- iri lane: outstanding (not reported); audit surface otherwise
  complete across all 52 crates.

## wave-9 last lane (iri) — audit surface now complete across all 52 crates

- lib.rs:6 claims one dependency; purrdf-hash is a second (README is
  correct). README re-export list omits json_pointer and percent.
- vocab gap: the SHACL-JS term family (sh:js, sh:jsLibrary,
  sh:jsFunctionName, sh:jsLibraryURL, sh:JSConstraint, 
  sh:JSConstraintComponent, sh:JSExecutable, sh:JSLibrary) is used as
  literals in shapes/src/spec/census.rs with no vocab.rs constants —
  the one real namespace-coverage hole (extends OPT-72).
- langtag errors carry no position (fieldless Copy enum) while the
  crate ships LineIndex/Position for IRIs — rule named, location not.

## wave-10 semantic additions (gts reader — all reader.rs)

- Invalid-header segment reuses the previous segment_index (sink
  events from two segments indistinguishable; ~:2152/:2297).
- expected_head silently unverified on the SegmentBoundary fatal path
  in BOTH pre-segment early returns (:1760, :2277) — a pinned caller
  gets no TruncatedLog and no skip notice.
- h_blob_frame: a decoded payload that is not bytes drops silently
  (empty Ok(_) => {} arms at :1148/:1234) — no opaque node, no
  diagnostic, unlike every sibling failure.
- address_exhausted path (:2239-2251) discards the TornAppendError
  marker and abandons the open segment without finish_into_result.
- is_header_item (:1471) classifies any map with "gts" and no "t" as
  a header — mid-segment damage can silently fork the prev chain.

## wave-10 semantic additions (rdf-core mutable.rs)

- OPT-50 base index matches only top-level TermRef::Blank (:626-641):
  blanks nested inside base quoted triple terms are skipped, while the
  delta path recurses — completeness gap vs the pre-OPT-50 full scan.
- holds_rows (:1313-1318) probes base first and delta only on miss,
  but graph_rows is keyed by MutTermId — a delta graph resolving to a
  base term answers from the base key's missing entry (false despite
  live rows); withdraw_graph_declaration's RememberEmpty gate trusts it.
- holds_rows doc claims base-declaration answering; code returns false
  on missing graph_rows — masked only by both callers' pre-checks.
- withdraw_graph_declaration Implicit branch (:1304-1306) drops a slot
  while rows remain, contradicting its own doc.
