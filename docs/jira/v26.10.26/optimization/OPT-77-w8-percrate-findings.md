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
