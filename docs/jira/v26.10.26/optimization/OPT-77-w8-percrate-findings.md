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

## wave-10 semantic additions (text + sql compiler — HIGH-CONFIDENCE LOGIC BUGS)

- text analysis.rs:654-666 — unspaced-script fallback shatters ADJACENT
  Latin too ("abc漢字def" → a b c 漢 字 d e f instead of abc + per-cluster
  CJK + def); the per-cluster split must scope to unspaced runs only.
- text ranking.rs:624-638 — field-population-absent fallback mixes
  corpus.documents (N) with field-only token totals: length
  normalization divides by the wrong average and matches NEITHER
  declared fingerprint string.
- text analysis.rs:1223 — letter_sides probes skip combining marks but
  not removed controls/ZWJ; soft-hyphen-adjacent periods split
  inconsistently with the documented letter-period rule.
- sql compiler.rs:2420/:2430 — blank-node key `_:_{label}_{scope}` is
  an ambiguous concatenation (label "a" scope "12" == label "a_1"
  scope "2"); distinct nodes collapse.
- sql compiler.rs:2462-2479 — is_object built only from `_:`-prefixed
  objects: a nested statement referenced via a real IRI is not
  excluded and the smallest-subject tie-break can root at a nested
  sub-select; the tie-break also orders on scope-varying synthetic
  keys, defeating claimed determinism.
- sql compiler.rs:3301-3390 — inline_select never emits the parsed
  stmt.order_by: an inlined `ORDER BY x LIMIT 1` silently becomes
  unordered `LIMIT 1` (nondeterministic row).
- BM25F formula itself verified against the paper (no defect); IRI
  resolution verified clean (zero logic bugs).

## wave-10 semantic additions (shapes json_schema)

- violated_by (:6308-6311) treats a pattern COMPILE failure as
  satisfied: rdf:nil/true/false and sh:in integer members silently
  escape an uncompilable pattern, while sibling sites report loudly
  via ctx.pattern_error (:1331, :3998, :4063) — silent constraint
  loss, inconsistent refusal semantics.
- min_length > max_length admitted silently (:5530/:5839-5850): the
  emitted schema is vacuous (rejects every string) with no
  comment/ctx diagnostic, unlike the module's non-statable-constraint
  convention.
- min/max vacuity encodings asymmetric (:5847 vs :5841) — cosmetic,
  no wrong output; pattern conjunction verified order-independent and
  correct.

## wave-10 semantic additions (columnar, geo-kernel)

- columnar writer.rs:326-337 — dictionary cap i32::MAX while ids are
  i64: the i64::try_from error arm is dead and datasets with
  2^31..2^63 terms are refused though representable; build_terms_table
  re-does the conversion with an unchecked `index as i64`.
- geo-kernel cells: hilbert_path key layout, warp arithmetic, and
  containment boundaries all verified clean; one minor diagnostic
  mislabel (cells.rs:299-302 InvalidOutputLength reports consumed
  count, not declared length).

## wave-10 semantic additions (entail — RIF/named-graph silent incompleteness)

- rif/eval.rs:325-327 — an empty-body RIF rule is silently dropped
  instead of deriving its head unconditionally; non-ground heads are
  lost without an error.
- rif/eval.rs:190-200 — RIF seeding skips all named-graph quads; facts
  derivable solely from named graphs are never produced and never
  refused (silent incompleteness, not the boundary the codebase
  otherwise insists on).
- lib.rs:1155 — a premise whose imports are all satisfied internally is
  misclassified unresolved (false ontology-import-unresolved report).
- rif/eval.rs:363-371 — an interned IRI whose stored surface diverges
  from `<iri>` is classified as a literal, making the rule unmatchable
  (incomplete fixpoint, silently).
- engine.rs:391-394 — named conclusions matching default_conclusions
  skip stats.commit (tallies omit derivations) and the "same
  conclusion" dedup does not hold for chase-minted existentials.
- Termination verified sound: Datalog-bounded lanes + weak-acyclicity
  TerminationCertificate; cyclic collections refuse as MalformedList.

## wave-10 semantic additions (validate regime — 4 minor, core chain sound)

- parse_dl_certificate (:4555) hard-codes stopped:false and never
  parses a stop marker: cancelled vs cap-exhausted indistinguishable
  at the text boundary.
- service_knobs (:412-431) conflates Python/PythonText knob names for
  conclusion services while materialize_knobs distinguishes them —
  possible nonexistent-knob error names (UNVERIFIED against the
  binding crate).
- parse_regime_vectors (:1437) resets a body on a repeated directive
  while the sibling parser accumulates — malformed artifacts parse
  differently per reader.
- check_dl_proof_from_source (:4853) admits module-extraction
  certificates by starts_with(banner) — prefix-forgery passes the
  gate; only verify_prepared's receipt refusal remains.
- Core check chain verified sound (round-trip compare, storage
  cross-check, service×host exhaustive, negative-verdict coverage,
  consumer-side recomputation).

## wave-10 semantic additions (datalog seminaive delta iteration — SUBSTANTIVE)

- commit.rs:368-399 — withdrawn-row restoration re-admits at the
  ORIGINAL index, but the next round's delta span is [round_lo,
  row_count): a restored row can land below round_lo and is invisible
  to both the Delta scan and OldOnly — MISSED DERIVATIONS after any
  maintenance withdrawal mid-stratum.
- stratum.rs:181-195 vs :103 — evidence commit resets Delta::all but
  leaves first_round false, so zero-positive-body rules are excluded
  from the re-seeded round their own comment says depend on it; the
  differential oracle has the identical asymmetry (cannot catch it).
- stratum.rs:167-171 — an empty round ends the stratum even right
  after a Delta::all re-seed; sound only if evidence commit implies no
  new head facts, which Capture::commit_native does not guarantee.
- commit.rs:425 — committed_facts incremented on the re-proof path
  (telemetry skew, not soundness).
- seminaive.rs:4115-4178 — present_row probes with only
  positions[0]/[2] bound; sound iff partition_for's known_key
  partitions on predicate+graph with unique (subject,object) — needs
  one confirming look at Arity::KEY_POSITIONS.

## wave-10 semantic additions (sparql-eval joins, hnsw, xsd)

- binop.rs:2202 vs :2141 — the two EXISTS primitives DISAGREE at the
  undecided-evidence boundary (probe_evidence answers false,
  probe_has_match answers true over the same index).
- binop.rs:3405 — eval_minus service-partition path OVERWRITES
  removed[position] per partition instead of OR-accumulating: a left
  row visited by two partitions keeps only the last verdict.
- binop.rs:121 — truncated RIGHT SERVICE arm returns empty instead of
  joining left against rows in hand, contradicting the documented
  truncation law (conservative, not unsound).
- binop.rs:2375 — reserve_join_rows reserves 0 when remaining==0; the
  subsequent push grows via untracked realloc, bypassing the
  bounded-workspace accounting.
- binop.rs:3468 — schema-disjoint MINUS short-circuits before the
  evidence-tracking branch (evidence-lane inconsistency, not results).
- datalog seminaive SUBSTANTIVE pair recorded separately above.
- hnsw: level distribution, M/M0 boundary, repair, ef clamping — all
  verified clean (2 benign borderline notes).
- xsd exact tower: gcd sign, denormal rounding, cross-product
  compare, exponent caps — all verified correct across all four risk
  areas.

## wave-11 semantic additions (batch 1 — 8 lanes)

- sparql-eval update.rs: INSERT DATA ignores the configured bnode mint
  prefix (:552 vs :676/:1011); DELETE templates with blank nodes mint
  fresh per-row blanks instead of WHERE-binding (silent no-op delete,
  :675); self COPY/MOVE return before any governor poll (:1165/:1191/
  :1223).
- n3 parser.rs: escaped path operators bake literal backslash into IRIs
  (:2484); cross-scope quantifier re-declaration shadows instead of
  erroring (:2069 vs :2011); RDF12-mode @forAll/@forSome half-parses
  (:1107); @forAll-before-@forSome hard-refused though N3 imposes no
  order (:2051); spaced path ops against IRIs refused (:1772).
- hnsw search.rs: DistanceCache keyed by row alone — reuse across
  queries with different Targets returns stale memo hits (:106-163);
  Visited stamps indexed unchecked (panic on graph growth, :544);
  ef-guarantee silently degrades to full scan when the component is
  smaller than ef (:530).
- json decode.rs: IRI-composition subject collision (#vN/#sN against a
  document already containing a fragment) silently aliases an attacker
  node into a value slot (:60-84); codec-node Limit trips falsely on
  Structure-run counts (:159); dedup asymmetry across base/reifier
  tables (:309).
- rdf-capi query.rs: plain query_entry never clears result outputs
  (stale out_kind on error paths, :499-551); out_partial written
  BEFORE fallible store_result (:357-396); governed entry omits
  out_rows/out_graph from the null check — metered evaluation consumed
  then refused (:1268-1280).
- duckdb eval.rs (extends OPT-76): duplicate column names → colliding
  predicate IRIs (:238); envelope breaker discards the materialized
  prefix (bound non-deterministic in effect, :252).
- toml codec.rs: temporal lexical blindly T-replaces odd bytes
  (:239); float projection discards source lexical form (:169);
  selected_bound conflates byte budget with span-count cap (:351).
- slice cache.rs: dependency_closure silently drops unknown seeds
  (already ticketed) + physical-first ownership misreports valid
  owner-asserted terms (ownership.rs:374-420); Phase-5 parse failure
  swallowed → false StaleDependency (:531); slice_phase_leaf delimiter
  collision on \x1f-bearing paths (:448); stale-edge closure
  recomputation un-cross-checked (:219/:571).
- cdt ops/functions: clean (2 invariant-maintenance observations);
  markdown/slice/… lanes still landing.

## wave-11 semantic additions (sparql-eval path.rs — 2 REAL bugs)

- path.rs:675 — `?` is folded into the set-semantics `reach` lane:
  a path whose ONLY repetition is `?` drops the duplicate rows
  SPARQL 1.1 §18.4 bag-union requires (eval(P?) = eval(P) ∪
  {(x,x)}); `*`/`+` need ALP, `?` alone does not.
- path.rs:2752-2766 — negated property sets with mixed directions
  evaluate as UNION of directional scans instead of per-pair
  complement (`x !elts y ⟺ ∀i ¬(x elti y)`): pairs checked against
  only one list, direction-neutral complement endpoints never
  returned.
- Minor: ClosureFrame marks visited before the stop check then breaks
  without inserting (:1865-1875) — inconsistent with the evidence
  branch; Accumulate grew-exit keeps the walk alive on
  evidence-only churn (perf).
- Termination verified sound across Star/Plus/Range on cycles
  (visited-guarded BFS, per-level union, binary-power composition).
