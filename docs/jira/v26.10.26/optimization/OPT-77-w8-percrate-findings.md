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

## wave-11 semantic additions (sparql-algebra scope/walk)

- scope.rs:607-649 — visit_leaf_labels_with_memory drops LeftJoin/
  Lateral/Filter-EXISTS/Minus right-side blank labels (`_ => {}` arm)
  while Join/Union descend unconditionally: inconsistent label
  exposure under LabelSource::All.
- scope.rs:536-565 — spine-leaves descent through ANY Join returns
  composite nodes as "leaves": leaf ordering/identity depends on tree
  shape, not source order.
- walk.rs:54-68 — NodeRef::at_path walks all children per step (no
  early exit): cost-contract drift, not correctness.

## wave-11 semantic additions (rdf-core dataset, branchless oracles)

- dataset.rs:1505-1556 — a caller-supplied QuadProbePlan disagreeing
  with the pattern's bound axes hits expect/unreachable! (public API
  panic instead of a typed stale-plan refusal).
- dataset.rs:583-593/:1521 — graph-axis key `index as u32 + 1` has no
  overflow guard (u32::MAX wraps to 0, colliding with the default
  graph in permutation indices).
- dataset.rs:2107-2141 — cardinality estimate ignores pattern.g beyond
  existence (documented conservative, planner-only).
- segmented build.rs:819-836 vs format.rs:424-480 — the inline-fit
  budget constant is duplicated across two files (drift-prone block
  invariant).
- branchless-vs-oracle divergences (static): byte_lane_in_range with
  MSB-set hi (Rust 0 vs oracle nonzero, swar.rs:28 vs oracle.py:133);
  sort_small_by_key len>8 falls back to sort_unstable (ties can
  reorder vs the stable-oracle model, sort_small.rs:215);
  lower_bound_eytzinger oracle ignores sizes (untestable n/n+1 trap,
  eytzinger.rs:182); jaccard f32 double-rounding parity leans on the
  53>=2*24+2 theorem (bitset.rs:187).

## wave-11 semantic additions (xsd temporal — 4 findings)

- temporal.rs:1219 — leap seconds (second = 60, legal per XSD 1.1
  Part 2) refused outright by parse_hms: `2016-12-31T23:59:60Z` can
  never reach the comparator.
- rfc3339.rs:508-513 — LeapSecond::MonthEnd accepts :60 on the last
  day of ANY month (over-accepts March 31); the cited appendix allows
  only June 30 / December 31.
- temporal.rs:2317 vs :1781 — internal tz inconsistency:
  cmp_timeline_admitted returns an order for mixed-tz pairs beyond
  ±14h while instant_diff_admitted refuses the same pair — `a > b`
  can hold while `a - b` is Indeterminate.
- temporal.rs:1784-1791 — the ±14h indeterminacy window assumes xsd
  bounds, but the rfc3339 parser admits ±23:59 offsets: an
  RFC-3339-sourced instant can fall outside the assumed bound and
  yield a WRONG determinate order.

## wave-11 semantic additions (markdown slicer — identity instability)

- dialect/markdown.rs:524/:539 — ATX heading + doctrinal-divider
  recognition requires space/tab after the marker: a bare `#`/`⁂` at
  end-of-line is read as content, silently moving unit boundaries
  (the function's own quoted CommonMark clause says end-of-line
  suffices).
- split.rs:56-58 — overlap==0 newline cuts lose the separator byte:
  pieces' concatenation is not the unit's byte span (one byte lost
  per seam).
- claims.rs:475 vs identity.rs:164 — claim dedup collapses duplicate
  anchors while citation_iri hashes the DUPLICATED reified list:
  byte-identical graphs mint different citation node IRIs (identity
  encodes multiplicity the emitted claims deny).
- claims.rs:87 — section identity digests raw heading bytes including
  CRLF's \r: a CRLF doc and its LF twin mint different section IRIs
  despite the documented same-structure guarantee.

## wave-11 clean lanes

- ed25519 point/scalar: CLEAN (decompression ordering, Montgomery
  split soundness re-derived, comb completeness proven, no
  malleability gap) — security-critical surface verified.
- cdt ops/functions: clean (2 invariant-maintenance observations
  recorded earlier).

## wave-11 semantic additions (text index — 4 findings)

- index.rs:2606-2614 — postings sorted (graph, language, subject) but
  the contract documents (partition, document-id-ascending): any
  partition mixing languages across subjects breaks id order;
  merge/intersect consumers can mis-order rows.
- index.rs:782-790 — with_ranking_profile assigns `self.ranking`
  BEFORE validation and consumes self: on Err the caller's index is
  destroyed with partitions cleared, contradicting "returned only
  after validation".
- delta.rs:80-88 — with_delta's changed-set derives from the caller's
  operands, not the observed source; an omitted removal leaves a
  stale posting while digests still reconcile (removals never checked
  to exist).
- index.rs:921-951 — phrase_row_bound keys positions without the
  graph coordinate: cross-graph position coincidences inflate the
  bound (possibly intended per the comment, but a bag-of-rows bound).

## wave-12 semantic additions (governor kernel — rdf-core/src/governor.rs)

- ResourceVector::saturating_add_assign (:254) saturates at u64::MAX
  which IS the unbounded sentinel: an exhausted meter is laundered
  into "nothing spent" in GovernorEvidence::consumed.
- copy_insert (:842) .expect on checked_add — a crash path in a
  function whose contract is returning TryReserveError.
- SilencedEvidenceBuilder::insertion_bytes (:772) uses a max-estimate
  capacity while copy_insert's amortized growth can exceed it —
  admission-by-estimate, not proof.
- Empty-ledger-to-first-insert cost model inconsistent with
  admitted's actual allocation (slight over-charge case).
- Precedence-rank blocking and boundedness tests verified correct.

## wave-12 semantic additions (turtle stream)

- stream.rs:122 — base_iri validated EAGERLY for line formats whose
  grammar ignores base (N-Triples/N-Quads/HexTuples): an invalid base
  + valid bytes fails streaming where the buffered path parses fine
  (lane precedence divergence vs the doc's "ignores it").
- stream.rs:163 — the non-HexTuples arm's "incremental fold" is
  end-of-document: parser.finish() re-serializes and re-parses the
  whole accumulated set; failure surfaces only after full consumption
  (framing drift, not wrong data).
- stream.rs:299 — a per-line UTF-8 error leaves the failing bytes
  resident with offset advanced (harmless on abort; a resume consumer
  would double-count).

## wave-12 semantic additions (gts COSE_Encrypt0 receive path — 3 low-severity)

- cose.rs:149-151 — parse_encrypt0_owned accepts ANY tag value (or
  untagged) in the owned fallback while the borrowed path/encoder pin
  tag 16 (lenient-decode asymmetry).
- cose.rs:167-176 — decrypt0/recipient_kid resolve kid from the
  UNPROTECTED bucket only; protected-bucket kid parses to MissingKey,
  diverging from Sign1's either-bucket lookup.
- cose.rs:76-81 — decrypt0 never checks the AAD-bound protected
  header's alg == 3 (A256GCM): a peer envelope with another alg
  decrypts fine (auth holds via AAD) but dispatch is unenforced on
  receive while sign1.rs enforces it rigorously.
- Sign1 signing/verify lane verified clean (sig_structure covers
  received protected bytes verbatim; alg only from signed map;
  Ed25519-only resolver cannot downgrade composites).

## wave-12 semantic additions (purremb embedding, validate shacl)

- embedding/verify.rs ~460-500: inter-section alignment padding is
  never checked zero on read, though the writer always emits zeros
  and NO digest covers padding bytes — a junk-padding artifact passes
  all content digests (padding-zero invariant unenforced on read).
- embedding/writer.rs:1457 — zero-norm rejection tests norm == 0.0
  before finiteness: an overflow-to-inf prefix is misclassified as
  "invalid norm" instead of ZeroNorm (taxonomy, math correct).
- embedding/writer.rs:1406 vs verify.rs:539 — raw-path F32 hashing
  via raw_bytes vs verify's LE re-encode is byte-identical only on
  little-endian hosts (the one fold divergence; view.rs guards this
  elsewhere with target_endian).
- validate/shacl.rs: delegation boundary, no logic bugs (change
  targeting reads the MUTATED snapshot correctly; additions-before-
  removals settlement matches contract; one lossy error path at :402
  drops row context).

## wave-12 semantic additions (jsonschema validate, n3 printing)

jsonschema validate.rs (5):
- :1686-1689 — ref-chain counter resets on instance-address change: an
  alternating-address ref loop never trips MAX_REF_CHAIN (unbounded
  unless the exact (target,address) repeat saves it).
- :448/:1674 — cycle detection on raw pointer identity: temporaries
  (Instance::Name Rc) freed+reallocated at the same address give ABA
  — false ReferenceCycle errors AND missed real cycles.
- :159 vs :1134/:1138 — AnyOf absorbs all outcomes before the
  early-exit; correctness rests on a fragile !track coupling, not an
  explicit invariant.
- :1447-1458 — a regex resource error in PatternProperties stops() but
  the loop keeps applying remaining patterns (errored pattern treated
  as no-match before the error surfaces).
- :1568-1598 — failed UnevaluatedItems marks ALL items (including
  never-attempted) evaluated: a sibling unevaluatedItems can wrongly
  skip them.

n3 printing.rs round-trip (5):
- sanitize_blank (:569-579) merges distinct blanks ("a:b"/"a-b"/"a b"
  -> _:a_b) — identity loss on reparse.
- numeric_shorthand (:412-421) accepts "5." and "5 " as decimal/
  integer — prints bare, fails to reparse.
- @prefix name printed verbatim with no PN_PREFIX validation (:78/:86/
  :145) — ':' or non-ASCII emits unparseable output.
- rdf12_json maps Term::Var to "var_{sanitized}" blanks, colliding
  across vars and with genuine blanks named var_x (:674).
- Term::Var prints ?name outside any @forAll scope (:197-215) — not
  re-parseable N3, quantifier lost.

## wave-12 semantic additions (shapes engine/target_eval)

- target_eval.rs:210-236 — narrowing() applies node-level
  Class/HasValue/In narrowing without checking shape.path.is_none():
  on a property shape (reachable via sh:targetWhere) those bind VALUE
  nodes, not focus nodes — candidate set under-inclusive, violations
  silently missed (probable real bug).
- target_eval.rs:238-242 — narrowing offered only on direct
  properties_in children; Reference/Node recursion misses the
  referenced shape's property_narrowing (over-scan, not wrong
  answers).
- engine.rs:1201/:1231 — FocusNode dedup split between seen_foreign
  and seen_ids: the same IRI arriving via different target kinds can
  be validated TWICE (duplicate results).
- sh:not implemented correctly (conforms_memoized, no double
  negation); severity mapping verified sound.

## wave-12 clean lanes

- lex json/read.rs: CLEAN (escape boundaries, depth accounting,
  duplicate-member release all verified; 2 benign delayed-observer
  observations).
- shapes validate/shacl.rs (delegation boundary): clean.

## wave-12 semantic additions (columnar schema, xsd numeric)

- columnar schema.rs: no format-version marker anywhere (schema
  evolution fails silently; only golden digests pin it); positional
  column-order contract enforced by hand-indexing, no debug assert
  ties TableData.columns to TableSchema.columns (reordering compiles
  and corrupts); utf8=>ByteArray invariant test-only, not enforced in
  constructors; Table Ord/Hash order vs ALL dependency order
  coincides only implicitly.
- xsd numeric.rs:963 — parse_float accepts out-of-f32-range finite
  lexicals ("1e40"): Rust parser silently yields +INF, but XSD 1.1
  puts them outside xs:float's lexical space (invalid literal
  accepted).
- xsd numeric: promotion hierarchy (single-rounding fast paths),
  NaN compare/cast handling, and cast facet checking all verified
  conformant.

## wave-12 semantic additions (rdf-wasm query.rs)

- :1765-1767 — entailment closure limits take Option<u64> at the wasm
  boundary: a JS negative bigint (-1n) wraps to u64::MAX (silent
  "no limit"), the exact wrap decode_ceiling_message refuses for the
  six governor ceilings (which use Option<i64>).
- :2312-2329 — partial-answers Unknown maps to certainty "unknown"
  but forwards into_result() rows unconditionally: the "unknown ⇒ no
  row handed over" contract is enforced nowhere here (relies on the
  kernel returning None).
- :1869-1880 — update_governed validates maxAnswers on a DROPPED copy
  and stores the raw unvalidated args; enforcement depends entirely
  on the runner re-deriving via from_update_parts (if from_parts, the
  answer cap slips through).
- Row conversion order + consume-once bookkeeping verified correct.

## wave-12 semantic additions (sparql-eval engine pipeline)

- engine.rs:4170-4186 — the on-demand read's Limit stage skips the
  length check AND passed-increment for undecided-evidence rows:
  undecided rows are emitted after LIMIT is exhausted; the
  materialized lane applies LIMIT to every row (lane divergence).
- engine.rs/:4171-4176 — reaching LIMIT does continue-pull without
  setting ended: the cursor drives the relation to exhaustion (work +
  side-effect divergence).
- engine.rs:399-438 — reorder_like_admitted runs after
  certificate.split() on the Truncated branch: the truncation proof
  describes the PRE-reorder layout (certificate not recomputed).
- engine.rs:2846-2856 — Slice-over-Distinct accepted on the on-demand
  walk while dedup-keeps-first depends on pre-Slice order that lane
  never materializes (DISTINCT-under-LIMIT lane divergence).
- engine.rs:247-258 — publish clones text-order projection names over
  evaluated rows; safe only via a fragile rdflib.rs ordering
  invariant with no tying assertion.

## wave-12 clean lane (text corpus/segment/scratch)

- text corpus.rs/segment.rs/storage/digest/delta: CLEAN —
  resolve/digest preimages agree, radix rank accounting correct,
  scratch reuse safe (HRTB sink + error-path swap). Latent notes:
  reserve_for_query gated on dictionary while scratch consumers
  aren't; GraphFree::ID hard-coded literal vs the shared constant.

## wave-12 semantic additions (lex xml reader — 4 findings)

- dtd.rs:529-532 — entity_value decodes char refs INTO the stored
  replacement text: `<!ENTITY x '&#60;b/'>` expands to a live `<`
  that next() re-scans as markup; XML 1.0 §4.4 says char-ref-produced
  </& are literal data.
- Same flaw on the PE path: `<!ENTITY % p '%q;'>` stores a literal %
  but dtd_item re-scans it as a live PE reference (:259-267).
- reader.rs:1392-1397/:671-679 — end_tag forbids closing only at-or-
  below the ENTITY frame's entry depth: an entity closing an element
  opened BEFORE the reference is accepted (§4.3.2 well-formedness).
- reader.rs:844-854 — the char-ref `;` scan is unbounded: `&#` with
  no semicolon swallows to EOF/attribute end with a misleading
  offset instead of failing at the markup boundary.
- Namespace stack verified clean (resolve/mark truncation, reserved
  prefixes, attribute-vs-element order).

## wave-13

- rdf-core ownership.rs: CLEAN (0 findings).

## wave-13 additions

- rdf-core dataset_view: stats_fingerprint is (rows<<32)^terms — a
  remove-one/add-one swap collides, defeating the documented
  snapshot-staleness guarantee (dataset_view.rs:1126 +
  delta_view.rs:679); cardinality_estimate adds the base's ENTIRE
  non-ordinary mass to every base-matching pattern (useless for
  selective probes, delta_view.rs:732). GraphMatch membership, batch
  resolve, and snapshot immutability all verified sound.
- hnsw params/profile: footprint_bytes_32 omits construction/query
  workspace heaps (the 32-bit admission can under-count, params.rs:
  215-315); parse_parameters silently last-wins on duplicate tags
  (profile.rs:481); profile-refusal boxing failure re-types an
  admission rejection as Workspace/AllocationFailed (:520-530); the
  64/32-bit admission paths share no computed bound (validate_against
  discards the product it overflow-checked).

## wave-13 additions (cli convert)

- convert.rs:351-361 — the canonical lane skips
  resolve_target/refusals entirely: `purrdf convert in.ttl out.nt.gz
  --canonical` writes PLAIN N-Quads under a gzip-promising name (the
  file-lies-about-its-bytes class format.rs:124 exists to prevent).
- convert.rs:373/:393-405 — the entailment single-source lane never
  populates read_ledger: parse-side loss entries are silently absent
  from --loss-ledger on that path (emission asymmetry with the
  merged/GTS branches).
- convert.rs:437-441 — ViewCanonError::NotReady arm binds and
  discards the error (silent-drop shape the file elsewhere refuses).
- Base-IRI threading verified clean across all three lanes.

## wave-13 additions (jsonschema output, geo cover)

- jsonschema output.rs:244-255/:365-368 — Basic/Detailed outputs omit
  the annotations/errors key when the list is empty; Core §12.3
  requires the array member present (shape-compliance bug).
- geo cover.rs:235-260 — CellCover::contains refuses InvalidStoredLevel
  whenever ANY cover cell is deeper than the query even when a
  shallower region strictly contains it: over-broad refusal on
  level-mixed covers, predicate non-total.
- geo cover.rs:1308-1326 — coalesce can produce a parent above
  `minimum` partly from straddling padding: the receipt's level
  histogram misreports the emitted-level distribution.
- geo chart.rs:517-521 — the ±180 seam shortcut over-classifies
  approaching-but-non-intersecting cells as Straddling (over-inclusive
  only, safe direction).
- geo hierarchy.rs:230-243 — descendant_range count shift unguarded
  (safe via validate_level cap; downstream multi-cell sums unchecked).

## wave-13 additions (xpath_regex, csvw projections)

- validate xpath_regex.rs:149-154 — refusal-code dispatch is
  string-equality on the diagnostic code: a SHACL query legitimately
  failing with a resource-coded diagnostic is misreported as a limit
  refusal. RESOURCES (:121-130) is an 8-element hand-mirror of the
  non_exhaustive Resource enum — new variants silently missed (the
  test guards Limits size, not Resource exhaustiveness).
- csvw terms.rs:1203-1210 — selector_matches uses raw
  starts_with(prefix) with no IRI-boundary check:
  https://example.org/vocab also selects vocabulary#x subjects.
- terms.rs:1104-1115 — an unselected-graph reifier row is recorded
  TWICE (GRAPH_UNSELECTED + REIFIER_DROPPED) for one source record
  (ledger double-count candidate).
- terms.rs:1276-1288 — language tag matched byte-exactly: a
  non-lowercase source tag (@EN) silently reclassified
  OBJECT_UNREPRESENTABLE.
- terms.rs:982-989 — per-cell limit checked only on representable
  values post-filter_map: unrepresentable-heavy cells bypass the
  limit via the ledger.

- ed25519 keys.rs signing path: CLEAN (RFC 8032 clamp, nonce derivation, challenge domain separation, key acceptance, s-composition all verified).

## wave-13 additions (wasm shacl, iri base/curie)

- wasm shacl.rs: From<ShapesProductRefusal> hard-sets code:None
  (:1024-1034) — the same refusal surfaces with vs without a matchable
  code depending on path; to_js_string hides Some(code) when
  dimension is None (:1014); import-failure priority wholesale
  replaces an admission dimension (:1098) contrary to the doc's
  never-dropped claim; validate_to_sarif test bridge flattens coded
  refusals to prose (:255).
- iri curie.rs:100-110 — opaque absolute IRIs (urn:, mailto:) whose
  prefix matches a declared CURIE prefix are misclassified as CURIEs
  and rewritten (term silently changed).
- iri base.rs:210-220 — an absolute @base directive keeps its
  fragment (the empty-directive path strips one): a fragment-bearing
  base leaks #frag into every resolved reference.
- base.rs:837-842 — classify-vs-parse fast-path drift risk (property
  tests pin it today).

- gts composite.rs: 0 logic bugs (offsets/lengths verified; 3 minor robustness notes: hardcoded 32 seed split, implicit empty-context zero-init, verify diagnostic collapse).

## wave-13 additions (projections lpg)

- mapping.rs:316-352 — an rdf_type quad with Blank/Triple object
  falls through label and property branches into an EDGE whose
  edge_type is rdf_type itself, governed by includes_edge_type
  instead of node-type scope (unlike every other type statement).
- mapping.rs:194 — reifier rows are scope-checked against the
  REIFIER row's graph, not the reified statement's graph(s): a
  cross-graph reifier passes/fails on the wrong side.
- mapping.rs:222-228 — annotation rows filter only on
  includes_predicate, skipping includes_node_types gating; untyped
  nodes excluded from the statement projection are re-introduced via
  annotation sidebands.
- mapping.rs:129-146 — node_types index admits rdf:type quads by
  graph scope even when the predicate allow-list excludes rdf_type:
  node-type membership derives from statements absent from the
  projected model (documented intent, but couples scoping layers).
- Sink emission order (cypher/GraphML/CSV) verified clean.

## wave-13 additions (cli sink, sparql-eval expr)

- cli sink.rs:356 — the Native arm's DownstreamClosed returns
  Ok(empty LossLedger), discarding the serializer's partial drop
  counts; the results lane (:312-314) surfaces nothing — the two
  lanes disagree on --loss-ledger after EPIPE. Pack-arm
  DownstreamClosed reports success without distinguishing wrote vs
  wrote-nothing.
- cli sink/query: ASK-to-CSV/TSV refused only at runtime after the
  query executed (late failure, not wrong answer).
- sparql-eval expr.rs:1044-1067 — EBV forces a type error for ANY
  language-tagged literal, but per SPARQL §17.2.2 (RDF 1.1+) it is
  xsd:string-typed: FILTER("x"@en) should be true (non-empty), not
  row-dropping. CONFIRMED logic bug.
- expr.rs:1134 — xsd_integer_of_admitted maps out-of-i128-range
  integers to None (treated as type error): SUBSTR("abc", 10^30)
  errors instead of the XPath empty-string result.
- expr.rs:7059 — saturating_add on the substring window can
  under-count the window width near i128::MIN (hedged, boundary
  only).
- Three-valued logic, XPath substring window, NaN compare,
  lang_matches all verified correct.

## wave-13 additions (retrieval fusion_stream)

- :3849 — observe_exclusions re-checks is_final/may_still_name at ask
  time but not the U(x) > T budget condition: a candidate whose
  ceiling dropped to/below threshold still gets pending-stream
  lookups (contradicts the never-looked-up claim at :3843).
- :3379/:3449 — count_collision and last_contribution advance on
  declared-duplicate rows that fetch then DISCARDS: duplicate rows
  inflate collisions_observed vs the doc's adjacent-emitted-rank
  definition.
- :4394 — the indexed ceiling uses >= not >: an unfinished rival with
  U exactly equal to the best final score blocks emission permanently
  under a zero plateau (flag in case equality was meant to be >).
- Rank-contribution ordering, threshold block sums, probe ordering,
  tie-break agreement, ceiling bounds all verified clean.

## wave-13 additions (geodesic — PARTIAL depth, no confirmed bug)

- geodesic.rs:468-471 — direct_fixed gates on longitude < 180:
  exact-180-delta non-antipodal pairs denied the fixed fast path
  (conservative, output correct via fallback).
- geodesic.rs:473/:666 — a dead tier in the resume ladder when
  maximum is in [64,112): restart at bits == maximum, one failure
  exits PrecisionExhausted without the 112 step.
- :1749-1775 — the antipodal bracket straddle case relies on an
  unguarded astroid-seed invariant (no assertion).
- HALF-EVEN rounding verified decisive at the 0.5um shortcut;
  integral_fallback recurrence re-derived correct.

## wave-13 additions (wasm async state machine)

- async_query.rs:2893-2903 — deliver_governed cancels the job BEFORE
  validating seq: a stale/unknown seq cancels a healthy job whose
  outstanding effect is a different one.
- operation.rs:699-737 — RawWithContext shares Raw's fallback
  serializer: a RawWithContext job without jsonld options silently
  degrades instead of refusing the kind.
- async_query.rs:1616-1618 — the (Fault, _) arm hard-codes
  StopCause::Cancelled: a deadline/stack fault surfaces to SERVICE as
  a cancellation (mislabel).
- :1271 — a stale seq delivery returns Stale with no fault latched
  and the outstanding effect never answered (recovers only via the
  abandon timer).
- :1468-1503 — record_suspension counts Abandoned/Fault round-trips
  as productive wait (metric plumbing).
- Ticket transitions, expire mapping, run() latch verified coherent.

## wave-13 additions (shapes report.rs, wasm async — prior batch)

- report.rs:829-892 — Complete sort keys FLATTEN nested detail trees:
  two results with different detail topologies but equal flattened
  keys compare equal; complete-report bytes are acquisition-order
  dependent.
- report.rs:1793-1800 — ResultTuple (dedup key) omits messages and
  annotations while result_sort_key includes them: results differing
  only in message text collapse in conformance/equality comparisons.
- validate build.rs:472-511 — SARIF resultPath built from the
  RELABELLED term (report-internal _:dg0 labels), not the
  data-graph label the docs promise callers can join on.
- report.rs:2210-2217 — collides_with_minted accepts leading-zero
  indices (r01 == r1): spurious mint prefix, benign.
- Severity lanes verified clean.

## wave-14 additions

- cli glob_rdf.rs:188-193 — count counts QUADS not distinct objects:
  a repeated triple spuriously fails the exactly-one check. Also:
  count==0 error conflates "no match" with "term not in dataset";
  the g.is_none() default-graph assumption is unverified against
  loader behavior.

## wave-14 additions (rdf-core diagnostic.rs)

- to_json (:576-628) omits the native cause attachment entirely
  (structurally incomplete vs StructuredError/Error::source).
- Hash/PartialEq (:358-383) ignore cause while clone_with_memory
  preserves it: distinct-provenance diagnostics collapse in sets/maps.
- RdfLocation Display renders <unknown>:17:9 for pathless positions.
- retained_bytes (:438-460) silently reports unknown capacity on
  overflow and never charges the cause SharedOwned.

## wave-14 additions (cdt_agg, wasm dataset)

- sparql-eval cdt_agg.rs:258-269 — finish_workspace LAUNDERS a real
  constructor failure into silent empty "[]"/"{}" for a non-empty
  fold (only the truly-empty group should yield the literal).
- cdt_agg.rs:423 vs :250 — DISTINCT dedups by RDF term identity but
  map_constructor collapses by normalized key (last-wins): FOLD(
  DISTINCT ?k, ?v) keeps a different value than FOLD(?k, ?v).
- cdt_agg.rs:383 — split_at_mut(min) silently truncates a short
  links slice (arity bug laundered into all-null values).
- cdt_agg.rs:187-195 — MAX_ELEMENTS checked pre-normalization; the
  constructor re-applies its bound post-dedup (order inconsistency).
- wasm dataset.rs: roleRules entries wrap built-in roles as Custom
  (:233); SinkWriter::flush is unconditional Ok (buffered bytes
  silently dropped, :72); match_pattern drops declared-empty graphs
  from the source's graph-existence state (:903); abandoned
  generation reservation on the IRI-error path untested (:828).

## wave-14 additions (cli query.rs)

- :271 vs :234/:917 — --division threaded into evaluation options but
  NOT into either prepare_query_with_options call: parse-time
  constant-folding of `/`/AVG runs under the default policy.
- :428/:461 — --jsonld-options and --provenance-namespace refusals
  fire inside emit_result AFTER full evaluation (governed: after the
  trip report), missing from refuse_unenforceable_combinations — the
  module's own stated pre-flight doctrine.
- :441/:448 — canon-arrow is refused by the provenance check but the
  error text/comment describe the family without noting it
  (doc drift).
- :344 vs :942 — governor construction timing asymmetric between
  GovernedQueryOp (view-op) and EntailedQueryOp (run): latent
  two-site drift.

## wave-14 additions (gts agent_memory)

- recall (:677-680) — min_confidence silently drops claims with NO
  confidence even at threshold 0.0 ("below this threshold" excludes
  None).
- revise (:539-543) — superseded_by mints a fresh IRI for an
  unstored replacement claim: the wasDerivedFrom audit link is inert
  (the module's own comments warn about this failure shape).
- inline_or_digest (:1039-1046) — tool args/results over 4096 bytes
  silently replaced by a bare digest, indistinguishable from inline
  data in graph and ToolCallRecord (quiet data loss).
- store (:453) — claim subject is digest(text): identical-text claims
  share one subject, conflating per-claim provenance and suppression
  scope.

## wave-14 additions (hnsw relation)

- relation.rs:967-974 — scoped_rows_per_invocation omits
  guard.max_neighbours(): a scoped plan can get a larger per-
  invocation ceiling than the same relation declares unscoped.
- relation.rs:875-883 — the neighbour-bound branch cannot distinguish
  KNN_MEMBERSHIP_MODE from KNN_MODE: any position-0-bound pattern
  returns rows.min(1) regardless of the admitted mode.

## wave-14 additions (validate entail, hnsw relation, capi handles)

- validate entail.rs:259 — the binding-facing entry hardcodes
  RulesHost::Rust: rule-limit refusals across Python/Wasm/C name Rust
  knobs, contradicting the per-host contract.
- entail.rs:112-122 — with Some(xpath profile), EntailRequest limits
  still route through RuleLimits while the doc promises
  Limits::new()-bounded runs (bounded-differently UNVERIFIED).
- entail.rs:140-150 — imported documents merge into the root shapes
  graph's rule table (no RuleSource dispatch; possibly intended).
- entail.rs:134-135 — multiple parse errors flattened via join("\n"),
  losing structured positions.
- hnsw relation.rs:967-974/:875-883 — scope/mode bound asymmetries
  (recorded earlier this wave).
- rdf-capi handles.rs:51-61 — PurrdfDataset::handle returns an
  unbounded caller-chosen lifetime from a raw pointer (no tie to the
  originating handle: dangling-borrow trap for future callers);
  division const accessor skips the null check every sibling has.

## wave-14 additions (geo chart)

- chart.rs:101/:116 — Interval::compare returns PrecisionExhausted on
  mere overlap: the pole clamp and full-wrap fallback can abort
  (fail-loud, safe direction) instead of conservatively taking
  Bands::One; math_for_sources retry behavior UNVERIFIED. Band
  rounding direction, wrap widening, and normalization idempotence
  all verified safe.

## wave-14 additions (hnsw payload/guard)

- guard.rs:742-746 — verify_rebuild launders malformed/missing
  identity TLVs via .ok() chains into "not reassociated": a
  reassociated payload verified under Exact (ArithmeticMismatch
  contradicting the documented Ok(false)) or a wrong-law verdict.
- graph.rs:965-974 vs :752-755 — encode writes build-shape fields
  conditionally, decode reads them unconditionally under a
  shape-bearing arithmetic: a None-shape Recorded shifts every later
  field (spurious decode refusals).
- guard.rs:731-733 — PayloadUnavailable folded into Ok(false),
  conflating unavailable with tampered (the enum deliberately
  separates them).

## wave-14 additions (n3 lexer, csvw non-terms)

- n3 lexer.rs:588-601 — the PName dot-break list omits quote/angle
  chars: `:o ."s"` absorbs the statement dot into the PName (same
  class as the fixed digit-run bug).
- lexer.rs:621-633 — bare prefix/base/version match case-insensitively
  in ANY position: a term named `version` lexes as the keyword.
- lexer.rs:565-573 — PName backslash escapes accepted without
  validation (`:\q` silently admitted).
- lexer.rs:585 — the `_: :` colon split requires len > 2: Blank(":")
  minted instead of splitting.
- csvw metadata.rs:50 — table-to-manifest binding by exact string
  equality, no IRI normalization (valid pairings rejected).
- table.rs:369-379 — languages_compatible matches any prefix without
  subtag boundary ("en" matches "enx").
- table.rs:400-407/:metadata.rs:375 — rowTitles resolved over virtual
  columns and validated without the name_explicit check primaryKey
  gets.
- metadata.rs:1162-1164 — a table declaring `"dialect": null` gets
  the default instead of inheriting the group's.

## wave-14 additions (phonetic, cli describe/rules, jsonschema compile)

- text phonetic.rs: no live bug (dead SS arm, latent byte-vs-scalar
  bound ordering, apostrophe-then-mark gray zone — all noted).
- cli rules: rules --shapes and rules --check never add the import
  readers to refuse_shared_stdin (shapes_tools.rs:131/:320) — stdin
  double-read, silent partial/empty import table.
- cli rules:239 — stderr announces inferred().len() but a lossy --to
  target can emit fewer rows (announced count exceeds the file).
- describe path clean.
- jsonschema compile.rs: draft-07 exclusiveMinimum/Maximum compiled
  as numbers (:552-567) though dialect.rs supports the boolean form —
  valid draft-07 schemas fail compilation; negative bounds saturate
  via to_u64_saturating instead of failing (:~893); $dynamicRef
  anchor resolution is silently lossy (compiles as plain Ref, ~:498);
  min/maxContains ignored under non-Validation vocabulary while the
  standalone arm still validates them (inconsistent paths).

## wave-14 additions (shapes product, triple.rs)

- shapes product/mod.rs:970-971 — the writer certifies declared
  functions against HostBindings::empty() while restore-time install
  (certified.rs:218-224) assembles over the REAL host table: a host
  shadowing a graph-declared function passes the writer check and
  silently changes semantics at restore.
- product/mod.rs:926-934 — to_product_with_implementation_identity
  hardcodes EMPTY registries while binding a non-empty identity:
  rows 7-9 refuse at restore under the real host (silent footgun).
- certified.rs:222/:179-194 — install unconditionally replaces
  shapes.aggregates and the rebuild seam runs no identity check: a
  rebuilt product executes under whatever the restoring host wired.
- rdf-core triple.rs:412-415 — reifiers_for_pattern binary-searches
  reifiers assuming sorted-by-id rows; the sort lives only at
  builder.rs:3080, unasserted (one upstream reorder = silent missed
  rows). read.rs:505-511 — cardinality estimate takes size_hint on a
  FILTERED iterator (over-count when both ends bound).

## wave-15 verification verdicts

- CONFIRMED: text analysis.rs shattering (adversarial lane re-read
  the fallback loop — whole-word grapheme split, Han is ALetter under
  UAX#29 so "abc漢字def" is one word).

- CONFIRMED: update.rs bnode prefix (insert_data:553 literal None;
  fix requires threading cfg in — insert_data has no options param).
- REFUTED: hnsw DistanceCache cross-query reuse — every production
  call site creates a fresh cache per query (pub(crate) type, Query
  borrows it); the :169 doc line is an accounting note. Doc-clearer
  only, no reachable wrong-answer path.

- CONFIRMED: sql memo-after-validation (compile_turtle runs
  validate_ast_shacl + canonical_graph_bytes before the :2004 lookup;
  the :1920-1924 doc explicitly promises the opposite).
- REFUTED: json IRI collision — document_id is hash-composed
  (profile.rs:226-242, never caller-supplied) and decode re-runs
  analyze + rejects on model.id() != document BEFORE the node walk;
  the alias is unreachable.

## wave-15 additions (entail rif deep-read)

- eval.rs:62-70/:242-247 — by_surface maps each surface to only the
  LAST-interned id while .expect assumes round-trip fidelity: two
  distinct TermValues with the same surface (plain literal vs its
  xsd:string twin) yield wrong ids or a panic at emission. (Deepens
  the W10 IRI-surface finding into the literal domain.)
- eval.rs:316-327 — the unbound-head check runs BEFORE the empty-body
  return: an empty-body rule with a head variable is rejected as
  UnboundHead instead of the documented derives-nothing path.
- eval.rs:357-372 (recorded W10; now traced to literal fallback) and
  body-variable name-identity reliance (engine-level term identity
  unverified).

- rdf-core sink.rs: CLEAN (drain invariant held at every boundary; Tee all-or-nothing is documented contract).

## wave-15 additions (compat rdflib query.py)

- :223 — Result.bindings includes None for unbound vars; RDFLib OMITS
  them (parity property silently differs for OPTIONAL/UNION).
- :171 — ResultRow.__getattr__: projected variables named get/asdict/
  labels/n3/serialize are unreachable via row.<name> (tuple methods
  win attribute lookup).
- :111-114 — _read_result_source does a single read(): a short-read
  stream truncates the results document (no loop to EOF).
- :284 — serialize CONSTRUCT path forwards **args into
  Graph.serialize: provenance_namespace/query_hash land as unexpected
  keywords (disjoint arg sets, no guard).

- CONFIRMED (spec deviation, latent for common shapes): path.rs `?`
  routed to set semantics drops bag-union duplicates; observable only
  when P under `?` itself has multi-derivation duplicates
  (alternatives/sequences), not for a lone-predicate `:p?`.

- CONFIRMED (adversarial, full trace): datalog restored-row invisibility
  — original append coordinates immutable, row_slots monotone, restored
  index strictly below round_lo of the re-deriving round; both Delta and
  OldOnly scans classify it wrong; only round-1 Delta::all would rescue,
  and restores fire mid-stratum at R>=2. The session's highest-severity
  confirmed finding (missed derivations = wrong query answers).

- CONFIRMED: shapes narrowing() misapplies node-level constraint arms
  on property shapes (no path.is_none() guard; sh:targetWhere admits
  property shapes; excluded focus nodes never re-checked through
  conforms_with_id_depth → silently dropped targets). Exposure not
  unique to targetWhere (any path-bearing shape recursing into these
  arms).

## wave-15 additions (gts sign1 CRIT/kid lane)

- sign1.rs:495-498 — verify_resolved erases sig.kid for valid but
  non-UTF-8 kids (recorded binary kid silently lost).
- sign1.rs:465-470 — a composite envelope with a resolvable text kid
  is marked "invalid" (key-type mismatch) instead of "unverified":
  denial-of-validity on shared-kid setups.
- sign1.rs:403-406 — CRIT-listing KID with kid absent entirely passes
  validation vacuously; criticality never enforced against resolution
  behavior.
- sign1.rs:352-365 — CRIT checks label membership only; the required
  header's existence is verified later and independently.

- CONFIRMED: expr.rs EBV language-tagged literal — ebv_term detects
  language: Some(_) at :1040-1067 and returns Ok(None) before xsd
  resolution; both plan-time and VM paths drop FILTER("x"@en) rows.
  Confirmed against SPARQL 1.1 §17.2.2 (Jena/Virtuoso give true).
- V2 EXISTS-disagreement verification: 429 rate-limit, not yet run.

## wave-15 additions (text index delta, n3 reasoner backward chaining)

text delta.rs:
- :152 vs :186-194 — dataset_holds_statements computed from
  whole-source count on the incremental path but selected-predicate
  sum on the fallback (coverage bit path-dependent).
- :166-191 — resolve(None) skips the loop silently: a non-empty
  source records as empty; surfaces later as misleading
  SourceMismatch blaming the caller.
- :455-461 — a delta listing the same row in added AND removed is
  accepted as a fingerprint-equal no-op (contradiction silently
  laundered).
- :74-79 — fingerprint-equal early return advances coverage/version
  but keeps the stale source_digest (permanent drift on collision).

n3 reasoner.rs (backward chaining — SERIOUS):
- :3254-3260 — log:notEqualTo definitively-false guard uses resolve,
  not resolve_pattern: a compound with unresolved inner vars compares
  two identical unresolved lists → branch killed, valid solutions
  lost.
- :3588-3590 + :5596-5701 — the backward cycle guard shares its
  stack with negation-as-failure: a cycle-cut proof reads as "no
  derivation" → UNSOUND notIncludes success under recursion.
- :3655-3674 — the unsound empty set is then MEMOIZED in
  completed_backward_goals and replayed for every later caller
  (permanently poisoned answers).
- :3274-3284 — list:notMember definitive-failure evaluates eagerly
  on a stale partial list, contradicting the delay discipline.

## wave-15 additions (lex percent + callers)

- percent.rs core: CLEAN (char-boundary guards, checked length math,
  %2E-to-dot per RFC 3986 §6.2.2.2).
- CONFIRMED caller bug: csvw rdf.rs:478 — {#var} encodes with
  URI_TEMPLATE_RESERVED whose keep-set includes # and ?: a value
  containing # breaks out of the fragment instead of becoming %23
  (RFC 6570 {#} requires the fragment set).
- jsonschema registry.rs:261/compile.rs:501 — percent::decode errors
  silently swallowed (.ok()? ): a malformed fragment degrades to
  "unresolvable" instead of surfacing NotUtf8/Malformed (inconsistent
  with registry.rs:810).

## wave-15 additions (columnar reader rest)

- reader.rs:469 vs :499 — decode_quad_rows reused for Annotations:
  an ordering violation there is refused as "quad-like row order"
  (annotations get no distinct context, unlike reifiers).
- reader.rs:394-405 — langtag refusal checks only is_identity_folded,
  never the kernel grammar: a lowercase malformed tag passes the
  strict gate and surfaces later as generic Reconstruction instead of
  a row-bearing terms.lang refusal.
- reader.rs:296-305 — terminal UnresolvedTerm fallback unreachable as
  written; cycles always reported as CyclicTriple even through a
  datatype edge (conflated refusal reason).

## wave-15 additions (shapes engine dispatch)

- engine.rs:830-834 — claimants runs every focus node through every
  shape's sh:ask: one shape's bad target aborts claims() for the
  WHOLE focus set (blast radius = all unrelated shapes).
- engine.rs:434-466/:775-810 — foreign SELECT results indexed only
  via enumerated_foreign which invert never reads: nodes claimable
  solely through the ask loop; no index-level backstop if an ask
  silently returns false.
- engine.rs:63-68 — sort_focus_nodes reads rayon::current_num_threads
  >1 inside the worker pool: "serial" resolution still forks a nested
  parallel sort (oversubscription).

## wave-15 additions (cli shapes_tools)

- :320-340 — run_srl_check never refuses shared stdin over --import
  pairs (the run path does): rules --check with two '-' imports
  double-reads stdin; the second sees empty and reports a bogus
  syntax error, contradicting check_srl's own read-as-run contract.
- :392-399 — unresolved-import special case keyed on MESSAGE STRING
  matching; every other SrlError::Import misattributes the failing
  document to the root --srl path.
- :203/:243-245 — shapes-graph diagnostics printed only AFTER the
  full inference output is written: a refusable shapes graph still
  commits its inference graph (ordering inversion vs the module-doc
  contract).

## wave-16 verification verdicts

- n3 memoized negation: SPLIT — stack sharing CONFIRMED (unsound
  notIncludes under recursion stands); memoization REFUTED (the
  cycle-cut return exits before the cache insert; poisoned empties
  are never stored).
- capi rowcursor stale reads: CONFIRMED (next() never clears current
  on exhaustion; the only guard fires pre-first-_next).

- jsonschema ref-chain reset: REFUTED as "unbounded" — targets and
  addresses are finite, the exact-repeat scan covers the whole stack,
  so any path over |targets|×|addresses| trips ReferenceCycle. The
  real weakness is bounded blowup (alternating addresses defeat the
  chain cap up to that product).
- update DELETE-template blanks: mechanics CONFIRMED (fresh-mint per
  row, never WHERE-bound, deletes nothing) — but fresh-mint is what
  SPARQL 1.1 prescribes for template blanks, so spec-conformant;
  the gate asymmetry at :675 (insert-only) is the factual residue.

- gts segment_index reuse: CONFIRMED (invalid header leaves
  valid_header=false, finish_into_result skips the push, next segment
  reuses the index; no sink event distinguishes them).
- EXISTS primitives disagreement: CONFIRMED (mechanism + reachability
  via evidence_exec undecided rows; both primitives selected on the
  same entry state by evidence-mode alone). Caveat: probe_evidence's
  gate looks semi-intentional (interned.rs established_rows filters
  undecided as non-members) — the unguarded probe_has_match boolean
  is the side ignoring that model. No test covers the case.

## wave-16 additions (shapes profile, W15 wave-14 stragglers)

- shapes profile.rs:345-354 — Request::admit re-resolves with the
  thread-local current() limits when one exists, silently discarding
  the caller's explicitly supplied xpath_regex limits (contradicts
  resolve_xpath's documented preserve-everything clause).
- profile.rs:345 — with no caller override, thread-native limits are
  ignored in favor of Limits::default() (inconsistent threading
  between the two paths).
- profile.rs:403-410 — run() drops a captured runtime failure on any
  Err variant other than Execution/Shapes::Invalid/Processor:
  governor resource exhaustion can be silently swallowed.

- text index postings order: REFUTED — PartitionKey is (graph,
  language), so a partition never mixes languages; the comparator
  degenerates to subject order = id-ascending within a partition.
  All consumers operate per-partition. (The W11 verifier missed the
  partition key.)
- text segment/storage.rs: empty-dictionary offsets.len()-1 underflow
  (:190) and zero-length words stored but exact(b"") always None
  (:137-183) — CONFIRMED edge-case bugs; unvalidated sorted-offsets
  precondition noted.

- mutable.rs holds_rows: REFUTED — mut_id_of is base-first and the
  base interner is immutable, so a base-known graph value can never
  acquire a Delta key; the claimed collision scenario cannot arise.
  (The W10 verifier missed the id-assignment invariant.)
- entail rules.rs: CLEAN — all 6 OWL-RL tables match the independent
  SPEC_RL_78 transcription rule-for-rule (78 correct), 17 false-headed
  rules verified.
- jsonschema registry.rs: draft-07 fragment-only $id skips
  $schema re-declaration (dialect silently ignored, :596-599);
  non-boolean $vocabulary coerced to required=true (:362);
  absolute_without_fragment returns the raw caller URI not the parsed
  form (:493-497) — normalization-differing registrations both pass
  DuplicateResource.
- gts sign1 (sig_structure + payload framing): CLEAN for GTS
  production; 3 interner-strictness divergences vs RFC 9052 on the
  receive path (empty crit refused, IV+PartialIV coexistence refused,
  kid always emitted unprotected).

## wave-16 additions (jsonschema output rest)

- collect (:273-277) keeps only units with own error/annotation:
  intermediate failing units explained-by-children are dropped from
  the flat basic list (Core §12.2 lists every unit on the failing
  path) — PARTIAL, depends on upstream error guarantees.
- detailed() fallback for a valid no-annotation tree emits bare
  header() without the annotations member — second omit-when-empty
  instance (W13 was Basic; this is Detailed).
- condense (:359) filters error by !valid: a valid-subtree unit
  carrying an error silently loses it in Detailed while leaf()
  prints it (projection disagreement).
- built-stack alignment, Clone/Drop/PartialEq verified sound.

- sparql-eval solution.rs merge internals: CLEAN (compatible, union/intersection, shared_columns, in-place permutation all verified; witness.rs/service_endpoints.rs merge fns also clean).

- ed25519 lib.rs/keys.rs public verify paths: CLEAN (check order, negated-argument composition, clamping, canonical-borrow tests all verified).

## wave-16 additions (cli format.rs, ed25519)

- cli format.rs:124 — after stripping a transport suffix the payload
  stem has no extension: the user gets "no file extension" instead of
  "this is a compressed stream; pass --from" (misattributed cause).
- format.rs:171 — resolve_target orders the transport refusal before
  resolve: an explicit --to cannot suppress the refusal the way --from
  can on the input side (precedence asymmetry).
- format.rs:284 — Pack/Gts hard-coded consumes_base=false may refuse
  --base as inert when import_gts_events actually resolves against it
  (needs canonical-lane confirmation).
- ed25519 lib.rs/keys.rs public verify paths: CLEAN.

## wave-16 additions (rdf-core intern/canon, cli format rest)

- rdf-core canon.rs:1766/:1801 — under FlatAssertion in Graph scope,
  already_asserted probes the row's OWN graph while the emitted quad
  has g: None: probe-vs-emitted asymmetry enforced only by convention
  (candidate duplicate/drop divergence).
- global.rs:445-471 — insert_missing partial-fail leaves orphan arena
  bytes and an unused reserved slot (id consistency holds; capacity
  accounting claim overstated).
- TermId niche + interner hash/eq protocol verified sound.
- cli format.rs: transport-strip error-text misattribution +
  resolve_target precedence asymmetry + Pack/Gts consumes_base
  question (recorded earlier this wave).

## wave-16 additions (columnar writer rest)

- Resolver never prunes active on error exits (:272-278): later reuse
  of the same resolver misreports unrelated terms as CyclicTriple.
- Reifier quads: resolve_all skips the predicate AND build_reifiers
  drops quad.p without checking it is rdf:reifies (:137-140, :525-538)
  — a non-reifies predicate term silently vanishes from dictionary
  and output.
- BTreeSet row dedup discards duplicate multiplicity with no
  LossLedger entry, contradicting write's documented lossless
  contract (:515/:559 vs :24-33).
- Dictionary i32::MAX cap vs i64 ids (W12 finding, cross-confirmed).

## wave-16 additions (csvw terms.rs loss accounting — 4 findings)

- :1104-1115 — REIFIER/ANNOTATION_DROPPED recorded unconditionally
  even when GRAPH_UNSELECTED already covered the row (double-count;
  the quad path continues to avoid exactly this).
- :1057-1086 — quad loss classification ORs across overlapping
  tables (first representation wins): mapped-but-incompatible in A +
  unmapped in B misattributes OBJECT_UNREPRESENTABLE.
- :1091-1102 — non_empty_graphs includes quads whose content was
  dropped: a fully-lost graph never gets EMPTY_GRAPH_DROPPED
  (structural loss understated).
- :996-1004 — One-cardinality integrity check runs on the FILTERED
  representable list: a 2-object violation with 1 representable
  buries the second instead of surfacing the violation.

## wave-17 verification verdicts

- shapes ask blast radius: CONFIRMED (the ? on decide propagates out
  of claimants with no per-shape catch; kills the whole focus-node
  dispatch and subsequent nodes).
- shapes profile failure-drop: CONFIRMED (final arm returns Err
  unchanged, discarding the captured failure; governor exhaustion
  itself is checked earlier — the drop hits other concurrent runtime
  failures).
- update insert_data bnode prefix: CONFIRMED (literal None at :553;
  mutable_mint_prefix resolves it to ""; insert_data doesn't even
  receive cfg; tests set the option but never exercise this path).

- columnar resolver leak: REFUTED as reachable (the leak is real but
  the Resolver is function-local and dropped on every error exit; no
  caller reuses it). Latent hygiene only.
- canon probe-graph asymmetry: REFUTED — the g:None is Graph-scope
  projection (rows pre-filtered q.g == Some(graph)); probing
  Named(graph) is exactly the emitted quad's true graph.
- hnsw guard TLV: PLAUSIBLE/downgraded — the Exact fallback is
  documented ("anything else under Exact") and no silent pass exists;
  residual is a diagnosability nit (misrouted reassociated payload
  gets a misleading error class).

- gts blob drop: REFUTED — the Ok(_) arms are dead defensive branches; a non-bytes payload under a chain is unreachable (payload returns only Value::Bytes in both regimes; the Err path is fully instrumented).

## wave-17 additions (capi serialize, gts composite recipient, csvw package)

- capi serialize.rs: 3 latent items (callback refusal code only in
  message text; let _ = flush() swallows a future real flush;
  partial-bytes-on-abort contract gap). No confirmed bug.
- gts composite recipient/key resolution: CLEAN (fail-closed header
  path; no downgrade).
- csvw package.rs:316 — to_ustar byte-accounting invariant enforced
  only by debug_assert_eq! (release builds ship a mismatched
  archive_bytes on future writer drift); all other checks
  validate-before-commit.

## wave-17 additions (entail classify — incomplete-taxonomy edges)

- classify.rs:236-238/:351 — representative() scans only 0..=i and
  derive assumes rep partitions equivalence classes: with Unknown
  pairs in the matrix the closure isn't transitively closed, so
  rep(k)=j while rep(j)=i — class k's reduction edges vs i's class
  silently dropped from direct.
- classify.rs:111-115 — bases left Saturated even for Unknown pairs:
  claims() cites a saturation decision never made (misreports the
  deciding basis).
- classify.rs:355-364 — transitive reduction via strictly_below
  treats Unknown as not-below: on an incomplete matrix an i⊑j edge
  can be emitted "direct" despite a derived path; Certified::direct
  gates on session completeness, not matrix-wide absence of Unknown.

## wave-17 additions (aggregates clean, jsonschema unevaluated REFUTED, regime rest)

- sparql-eval aggregates/HAVING: CLEAN (empty-group synthesis, SUM/AVG
  empty-bag, distinct carriers, key cells, HAVING placement all
  verified).
- jsonschema UnevaluatedItems fill: REFUTED as a bug — with
  output=true the loop continues past a failure so every remaining
  index IS attempted before the fill; the fill can only affect
  annotations, never turn an invalid instance valid.
- regime.rs (beyond W12): repeated @input/@program directives silently
  reset slots (@case refuses — asymmetric); proof_body_bytes maps all
  non-InvalidDigit hex errors to "odd number of digits";
  render_dl_certificate emits a fourth completeness value
  (storage-refused) absent from the documented grammar.

## wave-17 additions (hnsw lib search accounting)

- search.rs:172-178 — DistanceCache::insert increments evaluations
  only after insert_admitted succeeds: on bounded-workspace
  admission failure the search returns Err with NO work count for
  the evaluations already performed.
- lib.rs:700-708 — search_rows_work_admitted uses the admitted cache
  even on the unbounded branch: behavior diverges from
  search_rows_work for the same query by capability flag.
- lib.rs:845-849 — greedy_descent nodes not marked visited before the
  beam; correctness holds only via the memo (ties to finding 1).

- rdf-wasm term.rs: CLEAN (post-order fold consistent, merge-mode scope reset uniform, JS conversion surface is String-only by design).

## wave-17 additions (text ranking internals, cli project/lift)

- ranking.rs:654 — the normalization<=ZERO guard fires even when
  field.weight==0: a zero-weight field can spuriously error a term
  whose contribution would be exactly 0.
- ranking.rs:770 — inverse_document_frequency accepts frequency==0/
  documents==0 (prepare_query issues IDF=ln2 and a positive bound for
  an empty corpus; bound and corpus validity disagree).
- ranking.rs:636 — the remaining-carriers check degenerates to the
  already-validated corpus bound for exactly the sparse-field case it
  was written for.
- ranking.rs:583 — the frequency_sum>total guard is a loose union
  bound; per-field population lets one field's df ride on another's
  token mass (weaker than documented).
- cli projection.rs:93 — project's carrier output bypasses
  refuse_transport_target: `--output out.ustar.gz` writes
  uncompressed USTAR under a gzip name (same file-lies class W13
  caught in convert).
- projection.rs:66 — a bad --assets -/--config - combination is
  diagnosed only after read_config consumed the caller's stdin bytes.
- ledger.rs:28 — lift's merged ledger uses bare fs::write: clobbers
  an existing ledger unconditionally, no refuse-existing policy.

## wave-17 additions (shapes constraints, csvw manifest)

- shapes constraints.rs: qualified arm clean; 2 unverified suspicions
  on eval_closed (:1677 scans AnyGraph — potential over-reporting if
  the contract is default-graph-only) and permitted-predicate TermId
  interning (:1664 — an unoccurring declared IRI could be skipped on
  id mismatch).
- csvw metadata.rs:2182 — expand_datatype's allow_absolute is never
  called true (both sites pass false): spec-legal absolute datatype
  base IRIs silently rejected, dead branch at :2239.
- metadata.rs:1718-1734 — template_property returns Some(String::new())
  on an invalid template, discarding the inherited fallback every
  sibling helper returns.
- metadata.rs:338-345 — column numbering by columns.len() shifts
  derived _col.N names after any skipped non-object description.

## wave-18 verdicts + findings

- columnar reifier drop: RESOLVED at HEAD — resolve_all now resolves
  quad.p; build_reifiers_table refuses non-reifies predicates via
  ProjectionReason::ReifierPredicate (verified independently). The
  production fix + refusal corpus landed in 6a7ff794. OPEN RUNG:
  golden_bytes.rs PINNED digests are stale for reifier-bearing
  datasets (8 of 10 must move) — regeneration needs cargo on a build
  box; the red test target is the only known surface of this fix.
- csvw ustar: CLEAN on framing/padding/LongLink/truncation; latent:
  write_octal silently truncates >8GiB sizes; b'L'-then-EOF drops
  the pending name silently.

## wave-18 additions (batch 2)

- jsonschema dynamicRef degradation: CONFIRMED (compile.rs:503-517 —
  percent::decode failure/registry miss/JSON-pointer fragment all
  yield anchor: None with no Err; validate treats it as plain $ref).
- gts replication.rs (4): id/prev chain advances through INVALID
  frames — corrupt mid-chain frames validate as heads/resume points
  (:216-233); aggregate_digest filter_maps away None heads (damage
  hidden from digest+count, :326-338); file_head nulls when the LAST
  segment is non-foldable (:410-413); unreadable-id frame validates
  prev:"" against a fabricated genesis (:185-187).
- validate build.rs SARIF (5): text_carries_all ignores direction()
  (:415-418 — directional message loses PROP_SHACL_MESSAGES+bag);
  region only built when line is Some — column dropped (:655-659);
  uriBaseId stamped on absolute URIs (SARIF §3.4.2 violation,
  :650-653/:754-756); invocation notifications clobbered (:288-289);
  O(results×rules) position scan (:742-744).

- columnar langtag gap: CONFIRMED (is_identity_folded only; "en-" passes; surfaces as generic Reconstruction, not row-bearing terms.lang — is_well_formed exists but is never called in columnar).

- cdt DISTINCT divergence: REFUTED — DISTINCT dedups the full
  (first,second) tuple; an exact repeat can never displace the
  last-wins winner. Key equalities coincide (CdtKey is term identity;
  RDF 1.1 simple-literal ≡ xsd:string already identified).
- hnsw admitted cache divergence: REFUTED — admitted(workspace) ≡
  default() on the unbounded branch (charge on None is a no-op);
  divergence only via bounded capability, i.e., exactly storage
  backing.
- canon Graph-scope probe: refutation CONFIRMED by second opinion
  (rows pre-filtered q.g == Some(graph); annotation/reifier loops
  carry the same filter).

- shapes eval_closed AnyGraph: REFUTED — Core data graph is dataset-wide by design (data.rs:68-77: projected IR flattens all quads into default, AnyGraph ≡ DefaultGraph); sibling Core probes use the same scope.

- csvw One-cardinality on filtered list: CONFIRMED (2 matching objects, 1 representable → no integrity error, row emits one value; the dropped object is only a ledger loss, no cardinality recheck downstream).

## wave-18 additions (entail chase region — 5 findings)

- engine.rs:587-593/:855-858 — chase routing keys on existential-head
  presence but the chased path never runs first_clash: a program
  mixing an existential clause with a clash clause routes to
  chase_graph and silently returns a closure instead of an
  inconsistency witness.
- engine.rs:842 — chase_graph bypasses
  chase_with_declarations' declarations.admit(program) NeverDerive
  gate: the documented protected/undecidable-head admission never
  runs on the chase path.
- engine.rs:849-877 — the witness filter compares fact.graph too: a
  conclusion whose only witness occurrence is the GRAPH position is
  dropped (surrogate_drops) though its s/p/o is representable.
- chase.rs:596 vs :590 — reaches() evaluated after add_subsumption
  widens adjacency, but special edges were collected pre-widening:
  the certificate can under-report violations.
- engine.rs:862/report.rs:132 — Unbounded termination would surface
  as termination: None (indistinguishable from semi-naive no-
  obligation); soundness rests on chase.rs:1591 refusing first.

## wave-18 additions (sparql-eval vm/mod.rs)

- :1264 — CoalesceNext latent: a bound non-interned Val::Value branch
  would be misread as unbound; correctness hinges on the compiler's
  unstated intern-always-for-top-level invariant.
- :361-379 — expression owner ptr::eq guard hard-errors on Copied
  re-run with substituted pointers (loud, but the coupling is
  unenforced by types).
- :1665-1676 — pop_values mid-drain error leaves a corrupted stack;
  benign only because every caller ?-outs (terminal).
- Dispatch/stack discipline, IN error propagation, suspend re-push:
  verified correct.

## wave-18 additions (segmented read, csvw language tag)

- segmented read paths: CLEAN of block-boundary/lifetime/binary-search
  bugs (cache-position hint invariants valid; reverse-block phase-2
  partition covers boundaries; pinned lifetimes hold). Two
  hardening notes: format.rs:871 expect on an authenticated-but-
  untrusted graph index; format.rs:211 div_ceil unguarded if a
  section width ever exceeds block_bytes-9.
- csvw language-tag byte-match: REFUTED — @EN fails loudly at
  ProjectionTerm construction (validate_inner enforces lowercase),
  never silently reclassified; both comparison sides are
  canonicalized-lowercase by invariant.

## wave-18 additions (jsonschema metaschema/dialect — 5 findings)

- registry.rs:362 — non-boolean $vocabulary values coerced to
  required=true instead of refusing the meta-schema.
- dialect.rs:88-119 — vocabulary required=false honored only for
  format; applicator/unevaluated/validation still activate (union at
  :360-373).
- registry.rs:329 — Draft07 short-circuit: a recognized-dialect
  $schema uses default_vocabularies() even when a custom meta-schema
  with its own $vocabulary is registered at that URI (never read).
- registry.rs:637-641 — UnsupportedDialect stores the raw $schema
  (with fragment) while metaschema stores the bare form; '#' stripping
  ad hoc in three places (no canonical normalizer).
- meta_set.rs:100-107 — non-string $schema treated as absent (scanned
  as 2020-12); cyclic $schema chains report against pending[0] not the
  cycle.
- Verified correct: 2019-09 Applicator incl. Unevaluated; $vocabulary
  read at the meta resource pointer; draft-07 $vocabulary ignored.

## wave-18 additions (lex cbor — 5 findings)

- decode.rs:713-723 — read_from reports Ok(None) for ANY Truncated at
  offset 0: a first item with head-read but payload-missing is
  silently classified end-of-stream (strongest finding).
- decode.rs:189 — Slice::take reports the truncation offset as
  bytes.len() not the read position (skews offset-based errors).
- encode.rs:111 — Simple(24..=31) encoded via write_head (f8 18):
  ill-formed per RFC 8949 §3.3, rejected by the crate's own decoder
  (unforced writer/decoder contract).
- encode.rs:140-175 — canonical() keeps duplicate map keys: emits
  maps decode_deterministic refuses (documented but reachable).
- decode.rs:462 — depth-cap arm precedes the indefinite refusal:
  misdiagnosis (Depth vs Malformed), no accept change.

## wave-18 additions (lex yaml read.rs — 4 findings)

- name() (:721-738) — anchor/alias names stop only at blanks/flow
  indicators, never at ':': *x: v yields name "x:" (UnknownAlias)
  where mainstream readers read an alias-as-key.
- fill_scalar explicit-key path (:1705-1709) — NonFinite converted to
  Value::String(key) when scalar_keys is on: ? .inf silently accepted
  as a string key, contradicting the blanket .inf/.nan refusal.
- key_of (:2286) — explicit-key null renders as "null" while implicit
  keeps source text: ? ~ → "null" but ~: v → "~" (same node, two keys
  under scalar_keys).
- plain_rest (:832-838) — tab-indented continuation silently ends the
  scalar and reparses as block structure at column 0 instead of
  raising the Tab error.
- Flow nesting + ?/: block slot machine traced clean.

## wave-17/19 repair-landing receipts (R/F lanes)

- R4 sql memo: hoisted; sibling compile_document ordering at :2279
  flagged for follow-up.
- R11 glob_rdf: distinct-object BTreeSet selection + Failure::Selection
  detail field (also :146 borrow fix, committed separately).
- R12 cdt_agg: CdtOutcome::Error propagates on non-empty folds.
- R15 jsonschema draft-07: exclusive bounds compiled as Kind-swap
  modifiers post-keyword-loop.
- R16/R18 columnar: predicate refusal + ReifierPredicate error variant
  + annotations ordering context + langtag grammar gate; golden_bytes
  PINNED regen = only red rung (needs cargo box).
- R19 entail rif: by_surface -> Vec<u32> multimap, emission expands
  every reading (no over-refusal; xsd:string twins are valid input).
- F1 datalog: restored_lo span-widening (the CONFIRMED missed-
  derivations fix); F3 n3: cycle-cut counter makes NAF defer instead
  of spuriously succeeding; F10 cbor: EOF/offset by construction.
- F11 cbor encode: Simple(24..=31) refused; canonical dedups last-
  wins — COMPILES CLEAN (make b-purrdf-lex exit 0, first warm
  verdict). Follow-ups: Value::canonicalize needs the same dedup;
  randomized test premise needs owner alignment.
- F13 SARIF: directional bag, column-only regions, uriBaseId relative-
  only, notifications extended.
- F20 n3 parser: cross-scope quantifier shadowing refused; RDF12
  typed error; either directive order accepted. README.md:38-39 doc
  drift flagged (owner: README).

## wave-20 repair-verification verdicts (adversarial pass on the fix wave)

10 repairs independently verified CORRECT, 0 BUGGY so far:
- EBV lang-tag (R1): CORRECT — no regression, right fields, empty
  lang-string → false per RDF 1.1.
- shapes narrowing guard (R2): CORRECT — as_property() is exact,
  property_narrowing unconditioned, node shapes preserved.
- sql memo hoist (R4): CORRECT — nothing feeds the key after the old
  validation point; store path intact; sibling compile_document
  ordering confirmed untouched.
- shapes ask catch (R6): CORRECT — per-ask match, Ok arms unchanged,
  # Errors doc matches.
- cdt_agg propagate (R12): CORRECT — non_empty before move, Data kind
  fits, empty path unchanged.
- draft-07 bounds (R15): CORRECT — numeric unchanged, exclusive-
  without-bound no-op, placeholder always overwritten.
- rif multimap (R19): CORRECT — cross-product expansion, loud refusal
  on empty slots, sort-driven determinism.
- gts segment_count (R9): CORRECT — valid-only streams index-identical,
  Default covers, no missed literals.
- columnar langtag gate (F5): CORRECT — "en-" refuses, multi-subtag
  passes, recursive path gated identically.
- F2 EXISTS alignment: landed with regression test
  (probe_has_match_treats_undecided_evidence_rows_as_non_members).

## wave-19 repair verification + bench-prep findings

- n3 lexer: PARTIAL — keyword-position fix landed (exact token match);
  dot-break/backslash/colon-split still open (B6).
- yaml reader fixes (F12): CORRECT — all four verified against
  neighbors (:721/:1716/:2300/:840).
- csvw One-cardinality: CONFIRMED + fix shape (B9) — check raw matches
  pre-filter, loss entries for cell_value Nones (terms.rs:983-1004).
- duckdb eval.rs projection (B19, extends OPT-76): Timestamp lexical
  form "N (Microsecond)" invalid xsd:dateTime (:342); f32/f64 Rust
  "inf"/"NaN" invalid xsd lexical (:315-330); Blob/Decimal/Date/etc.
  silently dropped (:348); NULL = triple omission (:266).
- n3 proof_writer (B15): used_prefixes never scans generated-rule
  terms (:819-846 — unparseable N3); premise resolution picks FIRST
  differing derivation not the used one (:327-334); RuleNumbering
  (label,line) collision (:536-560); unsubstituted Vars in emitted
  bindings (:463-507).

- validate xpath_regex.rs: CLEAN (RESOURCES table matches Resource::index order exactly; profile parsing/limits sound).

## wave-19/20 additions (B-lanes)

- n3 parser quantifiers: PARTIAL — cross-scope + RDF12 landed; the
  scope.started must-precede-first-triple error is NOT the removed
  @forAll/@forSome ordering (second verifier correct: either-ordering
  fix genuinely missing; EYE accepts post-triple quantifiers).
- shapes profile.rs (B8): the landed failure-drop fix REPLACES the
  original error wholesale (comment claims "join" — mismatch);
  governor-error early return skips take_failure (failure dropped
  there too); admit thread-local pass-through can yield
  XPathProfileConflict the caller never caused.
- n3 memory-bounded fixtures (B1, OPT-74 fix): LANDED — analytic
  closure counts replace fixture-time materialization (chain =
  n²+3n, peer = 6n; both derived from the bench's own docstrings);
  timed closures untouched; ladder unchanged. The bench should now
  run without OOM on CI.
- validate xpath_regex: CLEAN.

## wave-20 additions (B-lanes + C2 competitive research)

- sql compiler statement_kind (B20): is_object only collects "_:"-
  prefixed objects (full-IRI statement nodes win root selection);
  foreign_class fallback walks HashMap nondeterministically;
  from_turtle discards language tags; MIXED_DOCUMENT_ROOTS mislabels
  single-node defects.
- gts agent_memory (B18): min_confidence drops un-annotated claims
  even at 0.0; empty superseded_by mints inert empty-IRI; revise
  resolves only the LAST segment's table (older-pack claims
  UnknownClaim); digest_id folds file_len — ids layout-dependent, not
  content-stable.
- n3 parser ordering: second verifier REFUTES the "either ordering
  landed" claim — the must-precede-first-triple error remains and
  EYE accepts post-triple quantifiers; genuinely missing fix.
- shapes profile (B8): the landed failure fix REPLACES rather than
  joins; governor early-return skips take_failure; thread-local
  pass-through can yield spurious XPathProfileConflict.
- C2 COMPETITIVE RESEARCH (Jena): no official LUBM/BSBM numbers
  published — only qualitative claims + archived SDB-era figures.
  Third-party TDB2 load rates: ~45k-150k triples/s (Wikidata 16.7B @
  44.8k/s). ESWC 2023 evaluation placed Fuseki LAST among engines.
  Apples-to-apples requires: same hardware, same loader mode, same
  dataset scale points. THE BAR IS THIRD-PARTY MEASUREMENT, not
  vendor claims — a publishable purrdf number needs only to beat
  45k-150k triples/s on comparable hardware.

## wave-20 additions (C3 RDFLib research, B11 gts wire, B17 xpath clean)

- C3 COMPETITIVE RESEARCH (RDFLib): no official suite; parse rate
  ~13k t/s Turtle / ~21k t/s N-Triples (3.1M-triple WordNet, issue
  #1261); 20-50x slower than C rapper (~265k t/s). Community range
  10k-100k t/s. PyO3 comparison methodology: per-syntax parse rate +
  serialize + store round-trip + memory/triple + BATCHED FFI (per-
  triple crossings erase Rust's advantage). Reference:
  dice-group/triplestore-benchmarks (SP2Bench/BSBM/LUBM).
- gts wire.rs (B11): decode_sequence refuses on ANY error, so
  corrupt/bounded files report identically to torn appends — callers
  folding the "intact prefix" accept corruption as a partial write;
  the returned offset is the refused item's start, not proven torn;
  Limits refusals mislabeled TornAppendError.
- validate xpath_regex: CLEAN.

## wave-20 additions (batch 3 — B12/B14/B16 + sql bench landed)

- regime.rs parse_one_term: empty/whitespace input yields the scaffold
  quad and returns rdf:type as the "parsed term" (empty guard
  unreachable); decode not-recorded refusal requires trailing \n
  (message-routing defect); render unconditionally emits "availability
  recorded" (unenforced invariant).
- delta_view.rs: converted_annotations ignores its s/g arguments
  (cross-graph/subject leaks, :508-521); converted arms never mask by
  suppressed (:498/:519); withdrawn-and-redeclared graph policy seam
  (:906-912).
- sparql-algebra substitute.rs: seed-join descent stops at the first
  multi-child node — MINUS right arms and sub-query bodies see ?this
  unbound (escapes textual-substitution semantics, :15-17 vs :196-220).
- sparql-algebra walk.rs: assigned_prebound misses Group AS-outputs
  (latent, non-parser algebras); fold_post_order error paths may leak
  admitted storage (Drop ownership unverified).
- sql bench target LANDED: crates/sql/benches/compile_query.rs
  (cold/memo-hit/execute groups, testkit harness) — the crate's first
  bench (committed with the cbor syntax fix).

- shapes product host-shadowing: REFUTED — install runs verify_declared_functions against the assembled registry; cross-kind IRI collisions hard-fail in UserFunctionRegistry; declared-population fingerprint mismatch refused under UnsupportedCapability. Both restore seams covered (B7).

## wave-20 additions (sparql-eval property_fn_eval — 3 findings, deepens the W12 lane divergence)

- property_fn_eval.rs:4170-4178 — scoped-read LIMIT arm: undecided rows
  skip the full-check AND the increment — decided rows continue-pull
  forever while undecided keep passing; a scoped cursor can exceed the
  query LIMIT and never ends as is_full contracts.
- :1836-1857 + engine.rs:2237-2249 — FilterContext::context never
  applies options.division (materialised lane does): the same FILTER
  with division gives different answers on-demand vs materialised.
- :4136-4138/:4281-4300 — settled_search_evidence re-conjoins the
  relation declaration on EVERY pull instead of once at settle
  (evidence growth per row read).

## wave-20 additions (B2 sql bench full receipt)

- sql benches complete: compile_query.rs with sql_compile_cold/{64,240}
  (fresh WHERE literal per iteration, memo provably misses) +
  sql_compile_memo_hit/{64,240} (same doc, measures canonicalization+
  BLAKE3+lookup residual); Throughput::Bytes; sample_size(20);
  deterministic fixture derived from the sql_golden test.
- API gap disclosed: query-execute benches belong in the crate owning
  the DuckDB handle (purrdf-duckdb), not purrdf-sql — execution has no
  standalone surface in purrdf-sql.
- Makefile bug disclosed: `make b-sparql-eval` (without purrdf-
  prefix) targets a nonexistent package (exit 101) — pre-existing.

## wave-20 additions (cli node-expr/shapes_tools)

- shapes_tools.rs:461-464 — ExprFlags::spelled() has no
  --expr-turtle-file branch: diagnostics misname the operator's flag.
- shapes_source.rs:183-197 — a pair whose iri is already in root.loaded
  is still inserted: an import of the root's own IRI can resolve to
  the pair document instead of the root graph (root silently swapped
  out of the closure).
- cli.rs:1802 — --expr-at requires = "expr_via" makes the zero-step
  walk (At { via: &[] }) unreachable from the CLI.

---

## wave-21 verification audit (2026-10-28, 20-lane static fan-out)

Verification lanes over the earlier repair waves. Findings below carry
corrected anchors (wave-12's dtd line numbers had drifted).

### lex xml — wave-12 findings CONFIRMED UNFIXED, anchors corrected

- dtd.rs:528-532 (was :529-532) — entity_value decodes char refs INTO the
  stored replacement text: `<!ENTITY x '&#60;b/'>` yields a live `<` that
  next() re-scans as markup; XML 1.0 §4.4 says char-ref-produced </& are
  literal data.
- dtd.rs:261-268 (was :259-267) — same class on the PE path: a literal `%`
  stored in replacement text is re-scanned by dtd_item as a live PE ref.
- crates/lex/src/xml/reader/reader.rs:1391-1398 (was reader.rs:1392-1397)
  — end_tag forbids closing only at-or-below the ENTITY frame's entry
  depth; an entity closing an element opened BEFORE the reference is
  accepted (§4.3.2 well-formedness).
- dtd.rs:524-527 — the char-ref `;` scan is unbounded from `&` to the
  first `;` anywhere in the literal's remainder (the ledger's old
  reader.rs:844-854 ref was the attribute-value scan, which is bounded
  by `bytes[hit..close]`).

### datalog restored_lo — sound but over-wide; maintained model gap

- The span-widening repair is sound (Delta::contains lo-inclusive at
  seminaive.rs:1732-1735; restored_lo frame-local; no cross-stratum
  leak). But restored_lo is a running minimum never reset
  (stratum.rs:184), so every later round's delta span starts at the
  earliest restoration coordinate — O(R*S) re-scan plus per-round
  DeltaRun materialization on restore-heavy programs. The doc comment at
  commit.rs:56-57 says "the *next* delta's lo"; the code widens ALL
  subsequent deltas. Candidate fix: reset restored_lo once the round
  immediately following the restoration completes, or track exact
  restored coordinates instead of a floor.
- DEFECT CANDIDATE: maintained.rs never reads restored_lo —
  advance_until records it (commit.rs:419-420) but no maintained.rs
  reader exists; whether the maintained model's membership-delta
  machinery (maintained.rs:2032-2062) covers restored rows is
  UNVERIFIED. If not, restorations are delta-invisible under maintained
  evaluation.
- TEST GAP: no regression test covers restore-after-fixpoint delta
  visibility (grep of tests/ finds none). Belongs in
  crates/datalog/tests/rules_runtime.rs or a new
  tests/restore_visibility.rs asserting a derivation that requires the
  restored row on the delta side survives the fixpoint round.

### hnsw — work-accounting fix has no behavioral receipt

- The saturating-add-before-insert fix (search.rs:163-171, 1b6906fe) is
  test-blind: no test drives a bounded-workspace insert_admitted
  failure mid-search and asserts take_work() > 0 on the Err path.
  Nearest misses: oracle_contract.rs:383-398 is a planner row ceiling,
  sparql_e2e.rs:418 asserts the typed error but not the work count. A
  test in oracle_contract.rs using a tiny WorkspaceCapability ceiling
  closes it.
- Converting report-only benches to measured rows: harness capability
  EXISTS (testkit footers); the report-only design is deliberate
  (build.rs:14-24, thermal state). If measured rows are wanted, add an
  in-binary footer keyed on a non-wall-clock predicate (recall@10 floor
  at ef>=128, or per-scale digest non-zero/uniqueness — raw material
  already computed at recall.rs:304 and build.rs:170). Never wall-clock
  assertions (build.rs:150-154). Do NOT migrate to the sampled harness.

### entail — phase-4b rc=124 was run ordering, not a severed seam

- schema_scan's fixture is bounded (10 individuals, pairs ladder
  5/100/400/800); the consistency bench's expensive rungs consumed the
  3600s cap before schema_scan started. Fix is CI-side sharding:
  `cargo bench -p purrdf-entail --bench schema_scan` as its own
  invocation with its own ceiling. Optional static trim if a code edit
  is preferred: consistency.rs:573 drop the 64 rung (self-described
  scale probe), :600 drop 16 from the proof ladder, :446-447 consider
  dropping 16_000. Do NOT shrink schema_scan's own ladder and do NOT
  revert the achiever cache / range_by_role index (dl_step_ledger.rs:
  458-473 pins exact work at 36 for this shape).

### shapes — three verification gaps on the repair set

- TEST GAP: no pin of the guard's actual behavioral change — a where
  shape that IS a property shape (bears sh:path) and also carries
  sh:class/hasValue/in. Old behavior (narrow via constraint) and new
  (narrow only via minCount arm) both untested;
  target_eval.rs:238/241/244.
- TEST GAP: silent non-claiming of a failing ASK target (engine.rs:839)
  has zero coverage — neither the skip nor the promised valid-neighbour
  survival ("keeps the remaining shapes and focus nodes validable",
  comment :829-832).
- INCONSISTENCY: engine.rs:1987 (bind_complete_source) and :4667
  (parse_shapes_with_options) call resolve_xpath(options.xpath_regex)
  WITHOUT the crate::xpath::current() thread-local fallback that
  Request::admit applies (profile.rs:345-360) — with xpath_regex=None
  and an active dated native scope they select Limits::default(),
  diverging from the documented precedence. Same shape at rules.rs:876
  and engine.rs:3832 gating on is_none() only.

### columnar — langtag grammar arm untested

- reader.rs:409/:1083 `!is_well_formed_with(tag, Bounded)` refusal arm
  has no direct test: the only langtag refusal test is refused by
  is_identity_folded (uppercase), and generated tables only use
  "en"/"EN". A folded-but-grammar-invalid tag ("en-", "e-n") is never
  exercised in either direction. Valid neighbour exists
  (reader.rs:1213-1226).

### rdf-capi — post-exhaust rowcursor access untested

- The exhaust-clears-current fix (rowcursor.rs:117-120) has no test
  that calls purrdf_rowcursor_term AFTER CursorExhausted to assert
  InvalidArgument. Every existing test drains then frees (smoke.c:547,
  abi.rs:1586/1640/2277, query.rs:1931). Header needs no regen
  (contract already documented at purrdf.h:3279-3286).

### lex yaml — three test gaps on the new behaviors

- Anchor names breaking at `:` (read.rs:720-745): no invalid-case test
  anywhere (`&x:` / `*x:` forms untested); only near-miss is `*x : v`
  with a space.
- Tab-on-plain-continuation error (read.rs:843-849): no test feeds a
  tab-indented continuation of a plain multi-line scalar; existing Tab
  tests cover block indent and block scalars only.
- Null-key `~` convention: the lost-source path (alias-to-null key or
  popped single-pair key yielding "~", read.rs:2292-2303) has no direct
  test; literal-`~`-key rows exist (yaml_scalar_keys.rs:71).
- NonFinite keys: fully pinned both directions (value invalid-case
  tests.rs:409-411, key neighbours yaml_scalar_keys.rs:75-76) — CLEAN.

### sql update bnode prefix — one label-assertion gap

- The mint-prefix fix covers all eval-time seams (INSERT DATA
  update.rs:469-477/561-566, DELETE/INSERT WHERE :685-687, LOAD
  :1020-1022); parse-time seam does not exist by construction (PREFIX
  resolves fully at parse). Gap: no end-to-end "INSERT DATA with blank
  template honors configured prefix" label assertion — the INSERT DATA
  seam is exercised only via shared helper tests (update.rs:1603-1618).

### jsonschema / csvw — spot verdicts

- Ref-chain bound CONFIRMED ALIVE (validate.rs:56 MAX_REF_CHAIN=250,
  enforced :1686-1699; behavior tests api.rs:612/636/647). CLEAN.
- csvw loss accounting: all drop paths increment located,
  contract-checked ledger entries; exact-count arithmetic pinning
  exists only for the okf twin (okf_terms_projection.rs:78-124), not
  csvw-terms. Minor.
- probe_has_match gating: tests assert final evaluation results, not
  call counts. CLEAN.

### golden_bytes — 8 of 10 PINNED digests likely stale, no regen tooling

- PINNED table (columnar/tests/golden_bytes.rs:130-181) last pinned at
  de30c8f5 (2026-10-19); since then parquet.rs +1408 lines, segmented
  dictionary compression, reifier write-path changes all landed. 8/10
  expected stale (Blobs two likely stable — content-addressed). No
  #[ignore] regen function exists (contrast pack_container.rs:208-214's
  idiom); regen = run test, read assert diff at :195, paste, commit
  with reason per header contract. Needs a cargo box. Recommended:
  add the #[ignore] regen fn (static edit) so the cargo-box pass is
  mechanical.

### OPT-73 recount (supersedes the 84 figure)

- Real uninlined_format_args-class candidates: ~115 across
  n3/reasoner.rs (31), n3/tests/n3_parse.rs (22),
  n3/proof_check_n3.rs (22), n3/parser.rs (15), proof_writer.rs (6),
  printing.rs (5), proof_check.rs (3), lexer.rs (3), sql/serializer.rs
  (2), sql/ast.rs (2), duckdb/eval.rs (2), n3/ast.rs (1),
  n3/tests/n3_proofs.rs (1). Single-line grep; clippy on a build box
  is authoritative. Several ledger anchors already landed; count rose
  as unlisted files retained sites.

### misc wave-21 verdicts

- index.mjs PARSE_OK (node --check exit 0); ProvenanceInfo imported
  once, exported once; class-reexport test asserts static presence and
  it is present. CLEAN.
- purrdf.h needs NO regen for the rowcursor change (doc comments and
  signatures unchanged; cbindgen output would be byte-identical).
- Fanout residue: zero session markers, zero duplicate test names, no
  session-introduced debug leftovers.
- Commit-subject/file mismatches in the wave window (baeb5e4f touches
  only jsonschema/compile.rs under a shapes,sparql-eval subject;
  2017ec68 touches columnar+cbor under a jsonschema subject;
  47eb5ec9 touches cbor+yaml under an hnsw subject; 4ed3317b adds the
  sql bench under a lex subject) — bookkeeping drift, disclosed; do
  not rewrite history (fix-forward doctrine).

---

## wave-22 status pass (2026-10-28, exact-fix-plan wave + inline repairs)

### Fixed this wave (in-tree, UNVERIFIED — compile is CI-owned)

- slice cache.rs dependency_closure/product_unit: unknown seed now refused
  with the existing ManifestRefusal::UnknownCacheSlice (was a silent empty
  closure); merkle_root propagates; profile_closure_transitive updated to
  Result; two new tests (refusal + valid-neighbour).
- columnar writer.rs:393: dictionary-index `index as i64` replaced with the
  checked i64::try_from used by the builder (format is Int64 on disk; the
  i32::MAX refusal bounds the loop — one conversion idiom, no lossy cast).
- rdf-core dataset.rs: the two graph-axis `index as u32 + 1` sites collapsed
  into checked graph_axis_key/graph_axis_key_from_index helpers (key 0 =
  default-graph sentinel; a wrap would alias a named graph onto it); boundary
  test added (u32::MAX refused, top mintable index == u32::MAX).
- n3 lexer.rs dot-lookahead break set extended with " ' < > = # — `ex:foo."bar`
  no longer swallows quote residue into the minted IRI (B6 residue).
- toml parse.rs: two inverted error strings corrected ("a newline in a
  single-line string" / "a control character in a string"); no test pinned
  the wrong text.
- sql compiler.rs statement_kind: is_object now collects ALL IRI objects
  (matching document_roots) instead of only "_:"-prefixed; foreign_class
  fallback made deterministic (collect + sort) instead of HashMap-order
  find_map. B20 3-of-4 items closed (4th: MIXED_DOCUMENT_ROOTS already
  refuses by design; from_turtle language-tag discard still OPEN).
- cli projection.rs run_project: refuse_transport_target applied to --output
  (an .ustar.gz name no longer writes uncompressed USTAR under it).
- lex yaml tests: anchor-name colon rule + tab-continuation Tab errors +
  scalar_keys null-alias `~` spelling — three new test fns across
  yaml_reader_edges.rs / yaml_scalar_keys.rs (wave-21 test-gap closures).

### Wave-22 verdicts from the verification lanes

- xsd leap-second refusal (temporal.rs:1219): REFUTED-KEPT — XSD 1.1 §D.2.1
  says second < 60 and "leap seconds are not supported"; the repo's
  value_space.rs vectors pin the refusal; RFC 3339 lane owns :60 support.
  No change.
- sql memo (B-thrash finding): CLEAN — compile_turtle's memo key
  (BLAKE3 of canonical graph) covers the entire input surface; no options/
  profile/env affect output. No fix.
- datalog maintained.rs "never reads restored_lo": COVERED — maintained
  evaluation uses the term-membership delta (record -> changed/membership,
  maintained.rs:312-358), which captures restorations; restored_lo belongs
  to the stratum row-range path only. Defect candidate withdrawn.
- gts cose Encrypt0 kid: CONFIRMED (RFC 9052: bstr, either bucket; Sign1 is
  correct, Encrypt0 is unprotected-only + UTF-8-gated incl. the borrowed
  fast path; the single frozen vector doesn't exercise it). Fix is a
  6-site receive-path change — scheduled, not blind-applied to crypto.
- entail classify.rs taxonomy (representative() not transitive under
  Unknown; Saturated basis misreported on incomplete taxonomies; direct
  reduction ignores Unknown chains): CONFIRMED, fix plan recorded
  (union-find over pointwise equivalences + basis gating + Unknown-aware
  reduction). Needs its own careful lane.
- text index delta.rs four findings: CONFIRMED with exact fixes (coverage
  bit path-dependence; observe_source silent-else; contradictory added/
  removed rows cancel silently — needs ContradictoryDelta refusal;
  fingerprint-equal early return keeps a stale source_digest). Needs its
  own lane.
- csvw cardinality-One: CONFIRMED — One must constrain RAW values (code
  checks post-filter survivors; a raw-2->1-surviving cell passes and
  predicate_cell takes first()). Fix plan recorded; note the shared
  fixture() carries the raw-2 witness, so the fix must split the fixture
  or the success-path tests break.
- B18/B19/duckdb rows re-verified STANDING (lexical "N (Microsecond)",
  inf/NaN lexicals, silent drops, duplicate-column predicate collisions,
  min_confidence None-drop, empty superseded_by, digest_id file_len,
  last-segment-only revise). Fix plans in the lane receipts; scheduled.
- golden_bytes #[ignore] regen fn: lane died on rate limit; still open.
- OPT-73 count superseded by wave-21 (~115).

### Standing from earlier waves, now disposition-marked

- 12 R-lane repairs, 8 F-lane repairs, B8, and the csvw double-count and
  capi rowcursor fixes are CONFIRMED-FIXED at HEAD (wave-20 adversarial
  verdicts + wave-21/22 spot checks). DO-NOT-MARK exceptions: sql memo
  clear-on-overflow (compiler.rs:2033-2036), sql ast.rs serializer
  re-export (:6), ast.rs hardcoded sql# literals (:104-123), and retrieval
  fusion_frontier_alloc.rs stale prose (:144/:404) remain OPEN.

---

## wave-23 status (2026-10-28, implementation-plan wave)

### Fixed this wave (in-tree, UNVERIFIED)

- shapes: xpath::current() tier added to the infer_complete legacy fast-path
  gate (rules.rs:876) and the validate_with_focus_filter gate (engine.rs:
  3832) — an ambient native configuration now routes through the prepared/
  admit path instead of running un-admitted under a foreign configuration.
  Verdict table from the lane: bind_complete_source pre-flight (engine.rs:
  1987) = same-law earlier refusal (NOT yet applied — helper pending);
  parse_shapes_with_options = ambient-blind by design, DOCUMENTED, no change.
- text delta.rs: three fixes landed — ContradictoryDelta refusal (row in both
  added+removed, refused before mutation; new error variant + Display +
  StructuredError arm), fingerprint-equal early return now refreshes
  source_digest, missing selected graph on a nonempty next source is a loud
  TextError. Finding 1 (coverage-bit path-dependence) REFUTED by the lane:
  both branches derive the bit from the same whole-source quantity; the
  prior sketch's fix would have introduced the divergence it claimed to fix.
- sparql-eval row_checkpoint_gate.rs: three XSD literals swapped to
  purrdf_xsd::datatype constants (deps present).
- shapes free_expression.rs:224: rdf#value literal -> purrdf_iri::vocab::rdf
  ::VALUE.
- sql compiler.rs: memo flush-on-overflow policy documented (KEEP verdict:
  bench memo-hit group never triggers the clear; cold group is the honest
  flush-cost measurement).
- bench/LUBM_PROFILE.md: draw-order list completed (head index, research-
  interest draws, ResearchGroup count).
- cdt/Cargo.toml + iri/Cargo.toml comment drift corrected; helpers-ledger
  MIN_TOKENS floor + normalize.rs twenty->thirty + hnsw README order note +
  cli SUBCOMMANDS 16->20 landed in wave-22/23.
- columnar golden_bytes.rs: #[ignore]-equivalent regen function added
  (Trial::with_ignored_flag; the harness_main! macro cannot carry #[ignore]).
- xml dtd wave-12 premise CORRECTED by the lane: entity_value decoding char
  refs into replacement text is XML 1.0 §4.4/Appendix D CONFORMANT (incl.
  the tricky example, pinned by in-repo tests + xmlconf); the PE re-scan is
  likewise spec-required. Only hardening warranted: bounding the ';' scan —
  PENDING (10-byte window; same class at reader.rs:855 and dtd.rs:786).
- gts README/GTS-CONFORMANCE generated_by rows: WITHDRAWN — the prose
  describes upstream-governed frozen artifacts; not ours to correct.

### Plans banked (complete, verified, not yet applied)

- gts COSE Encrypt0 kid -> opaque bytes, protected-then-unprotected (RFC
  9052): full 6-site plan with consumer/test edits + writer-side
  keep-&str recommendation. Needs one careful crypto lane.
- entail classify.rs: union-find representatives (min-root union) +
  ClaimBasis::SaturatedIncomplete variant (8 downstream sites enumerated,
  wire tag 8, frozen digests unaffected) + reduction stays variant A once
  representatives are correct. Two full tests drafted.
- duckdb eval.rs B19: LexicalCell helper + rfc3339::format/xpath_*_text/
  canonical_hex reuse (NO second bodies; xsd dep addition required) +
  DuplicateColumnName refusal + unrepresentable counter. Tests drafted.
- agent_memory B18: is_none_or + EmptySuccessor variant + known_iri
  union-resolution + ID_SCHEME version gate (frozen hex must regen).
- sql ast.rs SelectStatement/SelectItem move into ast.rs (serializer
  imports); SQL_VOCAB_NS const (match-arm limits noted).
- check-gate-parity.py bounded-tier table + scripts/bounded-tier.sh
  (complete self-test suite drafted; workflow diff is 4 identical
  bounded()-body deletions).
- probe_evidence all-undecided-bucket mirror test (binop.rs) + early-exit
  REFUSED (record content feeds ledger/memo/rank attribution at every
  tier — O(bucket) is load-bearing).
- shapes bind_complete_source pre-flight via a shared
  resolve_xpath_selection helper (admit precedence single-sourced).
- sparql-eval fresh sweep (expr.rs/modifier.rs) and retrieval sweep died on
  rate limits — requeue.

### Wave-23 verdicts

- xml wave-12 "defects": 3 of 4 REFUTED (spec-conformant, vector-pinned);
  1 hardening item stands (unbounded ';' scan).
- text finding 1 REFUTED (see above).
- rdf-core/lex fresh sweep (term/builder/terminals): CLEAN — all casts
  guarded, no Result laundering, ring fence intact; two latent-fragility
  asserts suggested (builder.rs:180/:863 rollback skip, :3070 freeze drop).
- W24 lanes launched mid-wave: columnar reader/parquet, sparql-algebra,
  json, geo, datalog-negation, validate, rdf codecs, cli verbs, jsonschema
  numerics, n3 parser scoping, gts reader, testkit harness, python
  bindings, wasm js, python gates (incl. the windows-sys verdict), book
  truth, bench-correctness sample, OPT-70 single-sourcing (DELIVERED, see
  plans), and the live CI-verdict fetch.

---

## wave-25 status (2026-10-28, CI-truth + fix wave)

### CI VERDICT (gh, runs 40659364805 et al.) — read this first

- EVERY CI job fails on the pushed wave-21..25 commits, from TWO roots:
  1. **[patch] BLOCKER (pre-wave, machine-local)**: root Cargo.toml lines
     ~381-383 patch frozen-duckdb* to /Users/sac/... absolute paths that do
     not exist on CI — every cargo invocation dies at source resolution,
     before Format/clippy/tests ever run. Removing the [patch] changes
     resolved sources and needs a Cargo.lock regeneration on a cargo box;
     --locked builds fail until then. USER DECISION + cargo box required.
  2. **Format failures on ~45 pre-wave files**: bounded-fmt only covers
     changed files; the workspace Format step checks all. FIXED in wave-25:
     whole-tree rustfmt sweep landed (REMAINING-DIRTY: 0), commit ca2c4e72.
- Consequence: NO clippy/test signal exists yet for any wave-21..25 edit.
  The [patch] must be resolved on a cargo box before compile truth.

### Fixed this wave (in-tree, UNVERIFIED)

- Whole-tree fmt repair (see above) — the one CI failure class the wave
  edits did own.
- csvw terms: cardinality One on the RAW count (surviving-only was the
  silent-arbitrary-.first() bug); required kept on survivors (verified
  different semantics); shared fixture's unrepresentable witness moved to
  the parents column so ledger assertions hold without tripping the raw
  check. Implementation plan from the retry lane applied nearly verbatim;
  the dedicated raw-2 negative fixture/test remains to add.
- testkit harness: PURRDF_TEST_REQUIRE_EXACT dedups duplicate --exact
  filters (a passing suite was exiting 101).
- testkit bench: early-stop floor as sample_count.min(MIN_SAMPLE_SIZE)
  (behavior-identical today; intent-safe).
- gts agent_memory: min_confidence is_none_or (un-annotated claims pass any
  floor).
- lex xml dtd: char-ref ';' scan bounded to the 10-byte window (hardening;
  InvalidCharRef at the markup instead of scanning the literal's tail).
  The reader.rs:850 content-path ';' scan is bounded by markup structure
  and needs no change.

### New findings filed (plans needed next wave)

- shapes HIGH: duplicate sh:severity on one shape resolves by STORE ORDER —
  first_object_of over raw store order flips conformance on triple order;
  multi-valued severity must be refused (messages_of reads all values;
  severity_of drops extras silently). shapes.rs:2982-2989.
- shapes MEDIUM: conforms_from_dataset reads the FIRST typed report node
  while conformance_disallows_from_dataset unions ALL of them — a
  two-report dataset is judged under a law no single report states.
  report.rs:2607-2647.
- shapes MEDIUM: tuples_from_dataset first-object picks over AnyGraph +
  unwrap_or_default for absent severities — store-order-dependent tuple
  content and silent absent-vs-empty conflation. report.rs:2568-2592.
- shapes LOW-MED: reifier source refusals misattribute constraint index 0
  (occurrence: None by design -> unwrap_or_default). report.rs:1140.
- validate: column-only SARIF region is schema-invalid (§3.30 requires
  startLine/charOffset/byteOffset); diagnostic kind hardcoded None (=fail
  by default — Info diagnostics become failures); artifact URIs emitted raw
  (spaces/Windows paths break resolution); complete-JSON omits the
  conformance disallow set SARIF carries. build.rs:615-624/:656-663/:775-780.
- testkit (from the W25 L3 plan, partially applied): alloc-probe TLS
  try_with drops silently corrupt the live-byte ledger (plan banked:
  process-wide dropped-record counters + dropped_tls_records()); bench
  zero-match run exits 0 and --list miscounts (plan banked: record list
  outcomes + typed no-match failure; 118 bench_main! targets affected);
  iter_with_large_drop upfront iterations*size_of allocation (plan banked:
  chunked collection; 4 call sites); duplicate-id assert before --list
  dispatch (plan banked: report as failure in discovery mode).
- W24 sweeps died on rate limits: gts pub.digest HIGH (requeue),
  sparql-eval, retrieval, jsonschema numerics, from_turtle, OPT-75, doc
  book truth, bounded-tier verify (requeue).

### wave-25 addendum (same day)

- gts reader h_snapshot: id-shift overflow now substitutes the canonical
  refused u64::MAX instead of debug-panicking / release-aliasing an
  existing term (checked_add; downstream ForwardReference/Bounds refuse
  it with the same typed variants as any other out-of-range id). APPLIED.
- gts voided-frame plan banked (NOT applied — public diagnostic surface +
  test churn): a failed segment header currently voids the segment
  silently on the streaming path (process_frame early-return) while the
  buffered path folds nothing but emits one diagnostic. Plan: keep the
  void per spec §17 (silently misfolding is the forbidden outcome) but
  make it loud and sink-visible — new FoldError::VoidedFrame { frame }
  variant in the DamagedFrame class + per-skipped-frame sink.frame
  (valid:false) events + buffered-path parity loop. Exact diffs cited in
  the lane receipt; tests pinned by buffered==streamed diagnostic
  identity (Diagnostic PartialEq on code/detail/frame_index).

---

## wave-26 status (2026-10-28)

### Fixed this wave (in-tree, UNVERIFIED)

- gts reader: inline blob payload under pub.digest is now VERIFIED
  (digest_str(content) == declaration, else DamagedFrame + opaque) — the
  HIGH finding closed; reference-only lazy blobs unchanged by design. The
  one vector with pub+inline-d (22-inline-blob.gts) has a correct digest,
  so frozen vectors pass; a mismatch vector should be added upstream.
- shapes severity_of: multi-valued sh:severity refuses deterministically
  (count+first-well-typed loop; store-order conformance flips closed).
  components.rs:1147/:1250 share the first_object_of(SEVERITY) pattern —
  FLAGGED, not yet changed.
- validate SARIF: column-only regions omitted (schema-invalid per §3.30);
  rdf_kind() maps Note/Info -> informational (was silent fail default);
  artifact URIs percent-encoded via purrdf_lex::percent::PATH.
- testkit bench: --list records Outcomes (accurate tally), duplicate id in
  List mode = fail outcome not panic, zero-match selection = typed error
  exit (List keeps 0).
- testkit alloc-probe: TLS-dropped record ledger (TlsDropped +
  dropped_tls_records()); recorders track per-leg failure and charge the
  dropped counters instead of vanishing.
- CHANGELOG + design doc: SHACL allocation pins corrected to the test's
  authoritative figures (OPT-75 partially closed — the OPT-75 ticket
  should be marked LANDED).
- cli sink: GTS-arm expect_err -> typed CliError::Usage.

### Verdicts

- sparql-eval expr.rs numeric arms + modifier.rs aggregate folds: CLEAN
  (i128 saturation provably exact for its only consumers; SUM/AVG replay
  on unprovable merges; MIN/MAX tie behavior sequential==parallel; all
  expects documented-unreachable). No change.
- retrieval/hnsw scoring sweep: CLEAN (fixed-point scores, guarded
  denominators, consistent top-k, no float in ranking). No change.
- jsonschema numerics: multipleOf/exclusive exact; one arguable
  over-refusal — unknown `format` under a format-assertion dialect refuses
  the whole schema (2020-12 §7.2 language favors annotation); documented
  deliberate in-code, FLAGGED for a decision, not changed.
- shapes report readers (conforms vs disallows from different nodes;
  tuples absent-severity -> Violation conflation): CONFIRMED with full
  fix plan (the_report_node() helper + refuse multi-report; absent
  severity -> Err). Banked, not yet applied.
- sql from_turtle: RdfTerm::Literal lacks language/direction (TermValue
  carries both); verdict EXTEND — banked plan, 4+ consumer sites need
  pattern updates, not blind-applied.
- bounded-tier single-sourcing: script text verified against the real
  workflow (7 call lines verbatim, helpers confirmed); the lane's YAML
  replacement block was output-corrupted — the mechanical rule is
  prefix each verbatim call with `bash scripts/bounded-tier.sh ` and drop
  the 4 inline bounded() bodies. Apply next wave from the verbatim list.
- python bindings sweep: ring-fence NOT enforced by check-layers (only a
  Makefile cargo-tree grep that misses optional deps) — finding filed;
  QuerySolutions __len__ stale after iteration (mem::take vs rows.len())
  — CONFIRMED small fix, banked; GIL-held projection loop + canon.rs
  panic pair — banked; mutex-poisoning expects — hardening note.

### Still queued (banked plans awaiting application)

COSE Encrypt0 kid bytes (6 sites), entail classify union-find +
SaturatedIncomplete, duckdb B19 LexicalCell rewrite (needs xsd dep),
agent_memory B18 (EmptySuccessor + known_iri + ID_SCHEME), sql ast.rs
SelectStatement move, check-gate-parity bounded-tier extension, hnsw
RankedSearchRefusal seams, probe_evidence mirror test, shapes report
reader fixes, from_turtle RdfTerm extension, python __len__ fix.

---

## wave-27 addendum (2026-10-28)

### Fixed (in-tree, UNVERIFIED)

- datalog schedule.rs: stratify_rules no longer refuses every model-reading
  rule (closed self-edge + shortest_path(from==to) = always a cycle). The
  evaluation path was reachable only via a hand-built Schedule.
- rdf hextuples: empty sentinel + non-empty lang yields the language
  literal (was: @en silently dropped, exit 0); rdf:langString + empty lang
  refused at the reader with the row's line number (was: position-less
  freeze error later).

### New findings filed (plans needed)

- rdf/xml reader: rdf:datatype silently beats xml:lang/its:dir (parse
  accepts, freeze refuses with a position-less error; equivalent
  N-Triples docs are rejected at parse — cross-codec asymmetry);
  its:dir without xml:lang silently dropped (plain literal instead of
  typed refusal). rdfxml.rs:509/:792-810. Fix plan: consult
  language/direction before the datatype arm; refuse
  langString-without-language at the reader.
- jsonld: scoped-context validation resolves against base_url while
  expansion uses scoped_context_base (accept/reject divergence for
  relative scoped refs). compiler.rs:627-652 vs :158-176.
- jsonld: expand_iri requires the prefix flag for compact-IRI expansion;
  a simple term whose mapping lacks a trailing delimiter gets prefix:false
  and "ex:foo" wrongly fails. compiler.rs:1769-1776/:873-874.
- shex: reference-only shape cycles refused by check_structure while the
  engine implements coinductive semantics for exactly them (over-refusal
  of spec-valid schemas, structure.rs:251-278); dangling shape reference
  classified as Violation -> NOT makes any node conformant
  (validate.rs:713 — should be Operational); duplicate labels silently
  last-wins in validate (check_structure never called);
  negative-cardinality ShExJ fields silently clamped (matcher.rs:47-53 —
  should be typed refusal; -1 max = unbounded is legit, -2/min<0 is not);
  cyclic triple-expr inclusion passes structure but always fails at
  validate (misclassified class).
- datalog plan-vs-rules stratifier divergence on variable-predicate
  negation (documented disagreement missing) + untested repeated-unbound
  existential NAF semantics (seminaive.rs:2686-2692 doc vs probe).
- python bindings: check-layers does NOT enforce the purrdf-core PyO3
  ring-fence (no `external` key on the core row; only a Makefile
  cargo-tree grep that misses optional deps); QuerySolutions.__len__
  stale after iteration (mem::take vs rows.len(), query.rs:738/:743);
  project_artifacts holds the GIL across the whole projection loop;
  canon.rs expect/unreachable! pair breaks the promised ValueError
  contract.

---

## wave-28 status (2026-10-28, apply wave)

### Fixed (in-tree, UNVERIFIED)

- entail classify: union-find representatives (min-root union) +
  ClaimBasis::SaturatedIncomplete (tag 8; all match/encode/decode/trust
  sites updated) + the derives-branch basis gate. The plan's corrections
  from the verify lane applied (new current.rs site covered by wildcard).
- shapes report: the_report_node() single-node law; conforms_from_dataset
  -> Result; tuples_from_dataset refuses absent severity (kept
  Severity::Other preservation — the verify lane's ok_or_else would have
  ALSO dropped Other, beyond the stated finding; change withheld).
  Callers: conforms_from_ntriples, tuples_from_ntriples, in-file tests,
  recursive_fixed_points.rs.
- shapes components: severity_of_node() at both free-fn sites (validator
  + component), mirroring Parser::severity_of.
- CI: scripts/bounded-tier.sh landed; bounded-gates.yml's 4 inline
  bounded() bodies deleted, 7 call lines rewired (byte-identical receipt
  format; python-verified 4-body match + 7-line rewrite). gate-parity
  table extension still queued.
- python: QuerySolutions.__len__ = remaining (len - pos).
- layers.toml: purrdf-core external = [] — structural PyO3 ring-fence via
  check-layers --ring-fence (optional deps included). Makefile cargo-tree
  grep deletion queued.

### Verified plans banked, not yet applied (exact diffs in lane receipts)

- COSE Encrypt0 kid -> bytes: full 25-site diff list verified against the
  tree (incl. writer Encrypt0Options.kid decision + recipient map Value
  ::Bytes). One API decision recorded: kid stays String vs Vec<u8> —
  plan recommends Vec<u8> (RFC 9052 bstr both sides).
- hnsw RankedSearchRefusal seams: verified; narrow version recommended
  (Scan fn-pointer keeps work: 0; widening Scan is optional).
- duckdb B19: verified with corrections (Date32 -> Option; new error
  variant needs source()-arm note; unsigned family typed as
  unsignedByte/Short/Int/Long). ~160-line diff ready.
- agent_memory B18: verified (EmptySuccessor + revise trim-guard as ONE
  edit at :539; known_iri cross-segment union; ID_SCHEME v1 + frozen hex
  regen).
- sql from_turtle RdfTerm extension: verified (4 sites; direction type
  path purrdf_core::RdfTextDirection; data stops at RdfTerm — future-
  consumer preservation only).
- probe_evidence mirror test: already in-tree in corrected form (landed
  by an earlier wave) — CLOSED, no edit.
- OPT-72 completion map: sql compiler.rs XSD swaps need purrdf-xsd dep
  (dep-decision); agent_memory XSD consts swappable (DATE_TIME spelling);
  RDF consts dep-decision; 102/71/63/47 remaining hits in
  sparql-eval/shapes/rdf/rdf-core = per-crate follow-up lanes.

### Verdicts

- doc sync: sales-benchmarks.md vs receipts — ZERO drift through waves
  21-27; both red markers intact.
- book getting-started: all commands check out (0 wrong commands).
- columnar schema/compact/projection sweep: CLEAN (triple-layer identity
  enforcement; compact bounds sound; projection round-trip consistent).
- cdt sweep: 1 defect — "a"^^rdf:langString accepted as a plain typed
  literal (parse.rs:722-731) violating CdtLiteral's documented invariants;
  fix = refuse langString/dirLangString in the ^^ branch + CdtLiteral::
  typed. Banked.
- csvw/gts test-writing lanes died on rate limits — requeued.

---

## wave-29 status (2026-10-28)

### Fixed (in-tree, UNVERIFIED)

- sql compiler.rs: RdfTerm::Literal carries language + direction (read-seam
  discard closed; consumers unchanged, data preserved).
- entail owl_dl: integrate() completion-mark rollback on observation
  refusal (was: mark-first + bail left the edge suffix permanently
  unintegrated while reach/delta read it as current — replay could diverge
  from the decision procedure on a clash reachable only through the
  abandoned suffix).

### New findings filed (plans needed)

- gts_write.rs:362-367: out-of-i64 metadata Integer silently CLAMPED to
  i64::MIN/MAX during CBOR conversion (should be typed refusal
  rdf-metadata-integer-range). :349-354: suppression `by` resolves by
  first positional display-value match — bnode-label/IRI collisions bind
  the suppression to the wrong term (key on kind+label+scope or refuse
  ambiguity).
- capture_sparql_goldens.rs:143-203: reclassification never prunes stale
  sibling artifacts (.skip-multi persists beside a new .rows) — "no git
  diff on re-run" only forward.
- alloc-probe (self-inflicted, wave-25 ledger): partial TLS failure
  double-counts — succeeded legs stay in TLS while the dropped ledger
  charges the FULL call (lib.rs:208-230). Prescription: reorder legs
  live-first AND make the dropped charge piecewise (charge allocations/
  size only for legs that failed), so the ledgers stay exclusive. Also:
  metered() is not panic-safe (enclosing peak/trough destroyed on unwind,
  :748-760); open()/sample() use panicking .with while recorders promise
  try_with (:576-598); TempDir prefix check bypassable on Windows via
  drive-prefix names (temp.rs:161).
- cli convert.rs:351-361: the --canonical lane bypasses resolve_target —
  out.nt.gz / out.gts accepted for canonical output (name lies about
  bytes). :286-338: --import + --max-stored-facts/--max-join-steps
  silently ignored without --entailment, and --import legs make the --base
  refusal look satisfied (phantom legs). Residual transport-target gaps on
  non-RDF byte writers: entails.rs:303, shacl.rs:200, shex.rs:297,
  validate.rs:878, shapes_tools.rs:586/:653.
- packet-N latin1 sweep: ZERO packet-framed stdio harnesses exist — no
  live violation; latent risk noted at wasm-test-runner.sh if it ever
  grows stdin frame reads.

### Verdicts

- testkit process_enter/disarm_and_drain CAS protocol: CLEAN (monotone
  drain, AcqRel proof, no clobber window).
- alloc-probe achiever... n/a — entail owl_dl achiever_cache +
  range_by_role: CLEAN (immutable keys, partial-walk never cached,
  built-once from finalized Kb).
- stream.rs framing: CLEAN (CRLF buckets, offsets, truncated-frame
  refusal).
- cdt deep sweep (W28): one defect banked (langString acceptance).
- Rate-limit losses: csvw test, gts test, generated-coverage lanes —
  requeued.

### Banked verified diffs still awaiting application

COSE kid->bytes (25 sites + writer API decision), hnsw
RankedSearchRefusal, duckdb B19, agent_memory B18, cdt langString
refusal, bounded-tier gate-parity extension, Makefile cargo-tree grep
deletion, csvw/gts/alloc-probe test writers.

---

## wave-30 status (2026-10-28)

### Fixed (in-tree, UNVERIFIED)

- cdt parse: ^^rdf:langString / ^^dirLangString on non-language-tagged
  literals refuse (over-acceptance closed; CdtLiteral::typed guard still
  open — signature change). cdt parse.rs.
- cli convert: --canonical applies refuse_transport_target (the lane
  bypassed resolve_target entirely). convert.rs canonical arm.

### New findings filed (plans needed)

- iri resolve.rs:253 + :39-47: base fragment DROPPED whenever the
  reference has none (RFC 3986 §5.2.2: else T.fragment = Base.fragment) —
  identity-changing; same-document shortcut also builds fragment None.
  HIGH. The equivalence oracle test uses a fragment-less base, so it
  cannot catch this.
- iri curie.rs: contract emits CURIEs curie_prefix refuses (local
  starting //) — broken contract/expand round-trip, resolve returns a
  non-IRI verbatim; resolve/expand_curie treat an absolute IRI whose
  scheme collides with a declared prefix (doi:10.1234/x, urn:uuid:...)
  as a CURIE and silently rewrite it. Both should consult
  is_absolute/absolute_verdict.
- sparql-results json.rs/xml.rs: blank-node envelope encode has NO decode
  counterpart — enveloped labels read back as literal label text
  (injective write, non-surjective read); non-langString datatype
  silently dropped on language-tagged literals (json.rs:401-410,
  xml.rs:318-327) — round-trip not identity for that class; duplicate
  projected variable writes a duplicate-key JSON object third-party
  parsers collapse (json.rs:159-192).
- shapes sparql ext: parameterized target's sh:ask runs WITHOUT the
  parameter pre-bindings (engine.rs:441-459 + sparql.rs:129-146 —
  disagreement refusal on conforming data); eval_sparql_constraint
  public door drops ?failure/?message mapping (sparql.rs:174-201 vs
  :292-294 — door-dependent verdicts); {$b} message substitution renders
  internal minted bnode labels (components.rs:446-449 — nondeterministic
  report content).
- gts_write: silent i128->i64 clamp on metadata integers (:362-367);
  suppression `by` positional display-match collision (:349-354);
  capture_sparql_goldens stale sibling markers on reclassification.

### Verdicts

- lex yaml writer: CLEAN (shared resolver = writer output always
  re-parses as intended; closed style-fallback chain; escaping matches
  the reader's decoder exactly).
- sparql-results escaping/sink/stream paths: CLEAN (CSV/TSV quoting,
  TextSink stickiness, BOM, NCName gating all verified).

### Requeued (rate-limit losses)

text analysis, rdflib-shadow, OPT-70 generated coverage, csvw/gts/alloc
test writers, COSE/hnsw/duckdb/agent_memory application.

### wave-30 addendum — knn cursor findings (W29 L12)

- CORRECTNESS: scoped cursor ignores the row licence (`remaining`) for
  UNDECIDED rows — the ceiling decrement/skip applies only to
  !is_undecided() rows (invocation.rs:934, :951-960), so a scoped Search
  whose scope decisions are undecided emits all N candidates while the
  unscoped cursor on the identical call emits the licensed 1. The licence
  is not an upper bound whenever a constraint is undecided. No
  undecided-x-ceiling test exists (knn/tests.rs grep empty).
- COST: scoped open performs the full-space scan + membership distance
  even when remaining == Some(0) (invocation.rs:303-344) — breaking the
  documented/plaintest "zero ceiling does no work at all" invariant
  (tests.rs:1214); observations record a scan for a call that should do
  none.
- COST: rows materialized + workspace-charged BEFORE the bound-position
  filter and licence check (invocation.rs:941) — bounded workspaces pay
  for rows the licence discards.
- MEASUREMENT: scoped take_work reports only the candidates count; the
  N per-candidate scope.decide/into_evidence calls contribute nothing
  (invocation.rs:348-382) — billed work diverges from actual.
Clean: select_best heap + -0.0 fold, guard, ceiling-withheld rule, cursor
reuse-after-exhaustion, fbbf/bbff split.

### wave-30 addendum 2

- FIXED: sparql update.rs graph_op_move — MOVE NAMED TO GRAPH <h> no
  longer destroys <h>'s own quads (destination-native records were
  re-inserted value-identically, then removed by the value-keyed removal
  loop; they are now skipped). Spec: the destination keeps its own quads
  and gains the source's.
- Banked (needs its own lane — per-solution vs batched delete/insert
  ordering in delete_insert, update.rs:827-833, medium-confidence spec
  reading; the batched behavior diverges from SPARQL 1.1 3.1.3 sequential
  delete-then-insert per row).
- Gate-truth findings (W30 L9): check-changelog-handwritten exits 0 when
  git-cliff is absent (the one case the prose checks did nothing);
  check-serializer-rewinds + check-toolchain-pin can pass auditing ZERO
  files if the scope tree moves; check-shared-helpers has no ledger floor
  (an emptied helpers-ledger.toml stays green); fb-receipt.sh darwin
  fallback broken (BSD date lacks %N; flock absent on macOS);
  conformance-matrix --from-results still builds. All need fail-closed
  floors — plans are one-line guards each (quoted in the lane receipt).
- simd.md re-anchoring: 2 rows name nonexistent functions; ~105 anchors
  sampled 10/13 stale — needs one re-anchoring pass.

### wave-30 addendum 3 — capi FFI findings (W30 L6)

- FIXED: ASK null out_boolean refused (both arms — was Ok + kind set +
  answer dropped, caller read uninit); query_entry clears *out_kind =
  KIND_NONE before fallible work (stale-kind cleanup hazard closed).
- FILED (design-level, need their own lanes):
  (1) query.rs:204 set_division_policy is a plain non-atomic write racing
  concurrent readers of the same field on a handle documented Send+Sync —
  UB per the documented thread-safety table; fix needs an
  AtomicU8/Mutex-backed policy or an explicit exclusivity contract
  surfaced in the header docs + a runtime guard.
  (2) term.rs:453-462: caller-supplied view.term_id hits
  TermId::from_index(...).expect (panic -> PurrdfStatus::Panic on caller
  input) and no arena-range check precedes to_owned_term; fix = range
  validation -> InvalidArgument.
  (3) serialize.rs: a reentrant on_chunk callback can free the dataset
  mid-serialization (plain UAF, no unwind); fix needs a reentrancy guard
  or a documented-and-enforced borrow contract.
  (4) out_buffer unwritten on error in query_json*/xpath_regex entries
  (same stale-state class, different carrier).
- Clean: ffi_try/ffi_guard coverage, cursor pinning of the dataset Arc,
  null-array refusals, unknown-discriminant handling.

### wave-30 addendum 4 — hnsw build+distance sweep: CLEAN

No wrong-neighborhood defect constructible in scope: build commit is
total-order sorted (no worker/hash order reaches the graph); exact vs
fast tiers agree on ranking by construction (monotone partials, overflow
checked before bounds, cross-tier divergence handled by path refusal not
drift); VectorMatrix immutable after construction (no grow/handle
hazard); decode bounds every untrusted count. Recorded borderline (not a
defect): commit truncation can evict frozen upper-layer edges and strand
nodes at layers > 0 while repair covers layer 0 only (builder.rs:201-203)
— a declared recall-degradation mode; worth a doc note if recall SLOs are
ever published.

