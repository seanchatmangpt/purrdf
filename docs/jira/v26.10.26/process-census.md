# Cross-Ecosystem Process & Planning Architecture Census for `purrdf-process`

Read-only survey of six repos (`ex4pm`, `ash_ex4pm`, `beam4pm`, `ash_pplan`, `wasm4pm`,
`bcinr`) conducted 2026-10-29 to specify `crates/purrdf-process`: POWL 2.0 (`model/`),
OCEL 2.0 (`log/`), HTN/FOND (`synth/`), conformance (`conformance/`). All evidence is
file:line from the surveyed repos; nothing was edited.

## 1. Target Feature Matrix

### `model/` — POWL 2.0 structural layer

| Capability | Source of truth | Port target |
|---|---|---|
| POWL AST (leaf/operator/partial-order/choice-graph) | ex4pm `Ex4pmEngine.POWL.Node` (`lib/ex4pm_engine/powl.ex:23-35`); wasm4pm `PowlNode` enum (`wasm4pm/src/powl_arena.rs:387`, flat `PowlArena` :419) | `model/powl.rs` — index-based arena (`u32` child refs, no Rc/RefCell), per wasm4pm |
| Operators: XOR / LOOP / PAR / StrictPartialOrder / FrequentTransition | wasm4pm `Operator` (:261), `StrictPartialOrder` with `BinaryRelation` adjacency matrix (:338), `FrequentTransition` (TaggedPOWL, :288) | `model/powl/operators.rs` |
| ChoiceGraph (BPM25 Def. 1–3, ▷/□ sentinels, `empty_path`) | ex4pm `POWL.ChoiceGraph` (`lib/ex4pm_engine/powl/choice_graph.ex:34-89`); wasm4pm `ChoiceGraphPowlNode` (arXiv:2505.07052) + `DecisionGraph` (`powl_arena.rs:358-379`) | `model/powl/choice_graph.rs` |
| Language semantics (per-operator evaluate) | ex4pm `POWL.Language.evaluate/2` (`powl/language.ex:27-87`) | `model/powl/language.rs` |
| SESE / top-down decomposition (Alg. 1–3) | ex4pm `WFNetToPOWL` (`lib/ex4pm_engine/decomposition/wf_net_to_powl.ex`: `convert/1` :23, `partition_mg/1` :47, `partition_sm/1` :65) | `model/decompose.rs` |
| Petri net (place/transition/arc + soundness) | ex4pm `WorkflowNet` (`workflow_net.ex:79-439`: `validate_structure/1` :147, `verify_soundness/2` :253, `enabled_transitions/2` :395, `fire/3` :403); wasm4pm `PetriNet` (`models.rs:1069`), PNML parser (`pnml_io.rs:112`) | `model/petri.rs` + `model/pnml.rs` |
| OCPN (typed per object type) | ex4pm `OCPN` (`ocpn.ex:28-161`: `verify_token_conservation/1` :128, `project_and_verify_soundness/2` :161) | `model/ocpn.rs` |
| POWL↔Petri↔ProcessTree↔BPMN conversions | wasm4pm `powl_api.rs` (`parse_powl` :55, `powl_to_petri_net` :235, `petri_net_to_powl` :281, `powl_to_bpmn` :293) | `model/convert.rs` |
| Process trees (inductive miner, cut detection) | ex4pm `ProcessTree` (`discovery/inductive_miner.ex:44-47`, choice cut :166, sequence cut :220) | `model/tree.rs` (discovery may defer) |
| Order-preserving shuffle generation | ex4pm `POWL.Shuffle.op_shuffle/2` (`powl/shuffle.ex:39-54`); bcinr `shuffle_fisher_yates_branchless.rs:28` (kernel, unported) | `conformance/shuffle.rs` |

### `log/` — OCEL 2.0 telemetry layer

| Capability | Source of truth | Port target |
|---|---|---|
| Core IR: Event / Object / ObjectType / O2O / E2O, attribute change-history with point-in-time resolution | ex4pm `Ex4pm.EventLog/Event/ObjectRef/ObjectRelationship/EventRelationship` (`lib/ex4pm/ocel.ex:1-72`), `attribute_at/3` :32; wasm4pm `OCELEvent` (`models.rs:762`, dual OCEL 1.0 `object_ids` + 2.0 `object_refs` w/ qualifier), `OCELObject` :831, `OCELObjectType` :864 | `log/model.rs` — adopt both-dialects shape (wasm4pm) + attribute time-series (ex4pm) |
| Schema-tolerant JSON ingestion (1.x + 2.0 dialects) | ex4pm `OCEL.normalize/1` (`ocel.ex:103`, legacy `ocel:*` keys :189-318, ref validation :481); beam4pm `OcelSessionFacts.normalize/1` (`beam4pm_ocel_session_facts.ex:158`, dialect detect :167) | `log/ingest/json.rs` |
| OCEL 2.0 writer (byte-identity, gzip, chunked) | ex4pm `OCEL.Writer` (`ocel_writer.ex:26-48`) | `log/ingest/json.rs` (writer half) |
| NDJSON streaming parse | ex4pm `StreamOcel` (`self_conformance_reactor.ex:1-53`); wasm4pm `load_ocel2_from_ndjson` (`ocel_io.rs:539`), `OcelStreamParser` (`wasm4pm-conform/src/ocel/mod.rs:30`) | `log/ingest/ndjson.rs` |
| Validation: envelope, referential integrity, lifecycles, cardinality | ex4pm `validate_envelope/1` :415, `validate_references/2` :481; beam4pm `Ocel.validate_envelope/2` (`beam4pm_ocel.ex:101`); wasm4pm `validate_ocel` (`ocel_io.rs:188`), `validate_ocel_object_lifecycles` :445, `ocel_v2.rs` OCEDO/OCPQ Def. 2 invariants :107-132 | `log/validate.rs` |
| Indexing / traversal (current state = linear scan or ETS; the port should do better) | ex4pm `flatten/2` (`ocel.ex:128-160`), beam4pm `OcelAccumulator` ETS (`beam4pm_ocel_accumulator.ex:148-194`), `object_trace/2` (`beam4pm_ocel.ex:51`) | `log/index.rs` — inverted event→object + object→event CSR over `purrdf-branchless` bitset rows (the gap all three repos share) |
| Hash-chained log integrity (pi_hash/pi_parent sha256) | beam4pm `ZcodeOcelVerifier.verify/2` (`beam4pm_zcode_ocel_verifier.ex:67`, path :520); fixtures `test/fixtures/zcode_ocel/*.jsonocel` | `log/integrity.rs` — replaces ad-hoc sha256 with `purrdf-hash` BLAKE3 + sha3 |
| Deterministic event-id minting | ash_ex4pm `event_id/4` (deterministic sha256 `"ev_" <> digest`, `notifier.ex:560-569`) | `log/ids.rs` |

### `synth/` — HTN/FOND trajectory synthesis

| Capability | Source of truth | Port target |
|---|---|---|
| HTN domain: tasks / methods / preconditions / outcomes | ash_pplan `Workflow.Dsl` task/method entities (`lib/ash_pplan/workflow/dsl/extension.ex:12-38`), `Model` (`workflow/model.ex:12`); HDDL corpora `planning/*.hddl` | `synth/htn.rs` (RDF-projected from `htn:` graph, not DSL) |
| FOND domain: states/goals/transitions, strong vs strong-cyclic | ash_pplan `FOND` (`lib/ash_pplan/fond.ex:28-34`), `Synthesis.synthesize/3` → `{:error, {:unsolvable, mode, witness_states}}`, `PolicySwitch.select/3` (`fond/policy_switch.ex`) | `synth/fond.rs` |
| Policy validation + candidate lifecycle (propose→validate→enact) | ash_pplan `SA2A.Provider.propose/2` (`sa2a/provider.ex:22-33`), `PolicyCandidate.fond/2` (`sa2a/policy_candidate.ex:17-26`, `authority: :none`, `standing: :candidate`), `Action.Run` (`action/run.ex`, authority ceiling `:construct`) | `synth/candidate.rs` — carry the authority ceiling verbatim |
| Typed refusals | ash_pplan `SA2A.Refusal` (closed codes, `sa2a/refusal.ex:1-15`), `FOND.Counterexample` (`fond/counterexample.ex:8-62`, classes `:liveness/:deadlock/:identity/:shape`, witness states), `Action.Run.Refusal` (`run.ex:49-59`), `RuntimeContract.Refusal` | `synth/refusal.rs` — Splode-style closed enum → Rust enum |
| Execution receipts | ash_pplan `ExecutionReceipt` (`execution_receipt.ex:39-56`: plan_iri/run_id/status/duration_us/outcome_digest/repo/subject_sha/base_sha); standing ladder 10 states (`standing/ladder.ex:11-23`) | `synth/receipt.rs` — maps onto purrdf's existing receipt vocabulary |
| OCEL export of plan transitions | ash_pplan `ProcessEvidence.export/3` (`process_evidence.ex:89-113`), `LedgerOCEL.events/3` (`run_started/task_succeeded/task_undone/run_ended`); beam4pm `OcelPlanningEvent` (`types.ex:11888`) | `synth/telemetry.rs` |
| Prefix claim / observe-replan loop | beam4pm `Ferroplan.session_restrict_prefix_claims/3` (`beam4pm_ferroplan.ex:341`), `session_plan_valid?/4` :647; `PowlConformance.conform_observe_replan/6` (`beam4pm_powl_conformance.ex:185`, `deviation_policy: :evidence_only | :refuse_reuse`) | `synth/replan.rs` |
| Replan routing ladder | beam4pm `ReplanRouter.route/2` (`beam4pm_replan_router.ex:374`, `next_rung/2` :644, ladder :902-926) | `synth/replan.rs` (policy half) |
| HDDL→POWL / FOND→POWL bridges | beam4pm `Powl.HddlPowl.from_network/1` (`beam4pm_hddl_powl.ex:47`, transitive closure :172), `FondPowl.from_policy_outcomes/1` (`beam4pm_fond_powl.ex:58`) | `model/convert.rs` |

### `conformance/` — the turnstile

| Capability | Source of truth | Port target |
|---|---|---|
| Token replay + fitness | ex4pm `TokenReplay` (`engine/conformance/token_replay.ex`: superfluous-token freezing, `replay/2` :66, `fitness_from_counts/4` :108); beam4pm ART72 `replay_trace/2` + `fitness_from_stats/1` (`beam4pm_art72_conformance.ex:96/152`, pm4py underfed-fire convention, engine-free); wasm4pm bitmask replay (`conformance.rs:964`, u64 masks :56-59, ≤64 places only) | `conformance/replay.rs` — generalize the u64-mask fast path to bitset rows so the 64-place ceiling disappears |
| A\* alignments | ex4pm `Alignment.align_trace/3` (`alignment.ex:97`, `f = g + h`, sync 0 / log-model 1.0); wasm4pm `compute_trace_alignment` (`alignments.rs:109`) + `compute_optimal_alignments` WASM export :290 | `conformance/alignment.rs` |
| OCEL (object-centric) alignment | ex4pm `Alignment.align_ocpn/3` (`alignment.ex:246`) | `conformance/alignment_ocp.rs` |
| Prefix conformance w/ law table | wasm4pm `PrefixOracle::classify_prefix` (`prefix_conformance/mod.rs:281`), detectors D1–D5 :98, law table `law.rs:12-29`, batch/online parity :588 | `conformance/prefix.rs` |
| POWL soundness + commutation certificates | wasm4pm `PcPowl2Checker::{bind_certificate, verify}` (`pc_powl2/checker.rs:41/49`), `check_soundness` (`powl/conformance/soundness.rs:212`), `check_choice_graph_soundness` :246, footprints :36 | `conformance/powl_soundness.rs` |
| ETC precision | ex4pm `calculate_precision/3` (`etc_precision.ex:18`) | `conformance/precision.rs` |
| Order-preserving shuffle conformance | ex4pm `op_shuffle/2` (above); bcinr `bcinr-powl/src/compiler.rs:545` `bp_tcrv_validate_reachability` (bit-parallel, with oracle :1539) | `conformance/shuffle.rs` — replace ex4pm's enumerate-all-interleavings with bcinr's bit-parallel reachability form |

## 2. Schema & Ontology Unification (canonical prefixes)

Grounded in real structures found (no invented vocabulary; `PurRDF is NOT an ontology` —
these are caller-supplied config shapes for `purrdf-process` to consume):

```turtle
@prefix powl:  <https://w3id.org/purrdf/powl#> .   # from ex4pm/wasm4pm AST
@prefix ocel:  <https://w3id.org/ocel/2.0/> .          # real OCEL 2.0 spec vocabulary
@prefix htn:   <https://w3id.org/purrdf/htn#> .      # from ash_pplan ap: + ferroplan HDDL

# — POWL — nodes are index-addressed; edges carry poset order
powl:Node            a rdfs:Class .            # union of Leaf|Operator|PartialOrder|ChoiceGraph
powl:activity        a rdf:Property .          # label (silent = no activity, tau)
powl:children        a rdf:Property .          # ordered list for operators
powl:operator        a rdf:Property .          # powl:Xor | powl:Loop | powl:PartialOrder | powl:ChoiceGraph
powl:minFreq/powl:maxFreq                      # FrequentTransition (wasm4pm :288)
powl:orderEdge       a rdf:Property .          # partial-order precedence (source,target)
powl:delimiterStart/End                        # ▷ / □ sentinels (ex4pm choice_graph.ex:46-47)
powl:emptyPath       a rdf:Property .          # DecisionGraph empty_path (wasm4pm :358)

# — OCEL — align with the official spec terms the parsers already accept
ocel:eventType, ocel:objectType, ocel:timestamp, ocel:attribute
ocel:e2o             # event→object with ocel:qualifier (ex4pm EventRelationship)
ocel:o2o             # object→object with ocel:qualifier (ex4pm ObjectRelationship)
ocel:value, ocel:time                          # attribute time-series (ex4pm ObjectRef attrs)
ocel:piHash, ocel:piParent                     # beam4pm integrity chain, promoted to first-class

# — HTN/FOND — from ash_pplan ontology.ttl (ap:) + its task/method structs
htn:task, htn:method, htn:subtask, htn:capability, htn:precondition
htn:after            # dependency (ash_pplan Task.after → depends_on)
htn:outcome          # :ok | :authorized | :declined | :timeout | :unavailable …
htn:policy           # FOND policy state→action map
htn:mode             # htn:strong | htn:strongCyclic
htn:standing         # ladder states UNKNOWN→VERIFIED (ash_pplan ladder.ex:11-23)
htn:refusalCode      # closed enum from sa2a/refusal.ex + counterexample classes
```

Determinism rules: no fabricated defaults (object types, capabilities are caller-supplied),
fixtures use `example.org`, W3C/OCEL spec terms via a `vocab` module, never string literals.

## 3. Code Extraction Inventory

### From `wasm4pm` (primary Rust donor — port nearly verbatim)

- `wasm4pm/src/models.rs:448` — `ColumnarLog` (CSR, `Cow<[u32]>`, zero-copy); `:762-880` OCEL structs + `OCELRaw` re-sync pattern
- `wasm4pm/src/ocel_io.rs` (JSON/NDJSON load/export, validate, provenance traversal), `ocel_v2.rs` (OCEDO invariants), `ocel_flatten.rs`
- `wasm4pm/src/powl_arena.rs` (entire arena + node enum), `powl_api.rs`, `powl_to_wf.rs`, `wf_to_powl.rs`, `process_tree.rs`
- `wasm4pm/src/conformance.rs` (bitmask replay engine + `uses_bitmask_engine`), `alignments.rs`, `prefix_conformance/` (oracle + law table), `pc_powl2/checker.rs`, `powl/conformance/*`
- `wasm4pm/src/models.rs:1033-1069` + `pnml_io.rs` (Petri net + PNML)
- Fixtures: `bench_data/ocel20_example.jsonocel`, `fixtures/world/*` (golden), `fixtures/negative/n01..n14`, `vendors/pm4py/tests/input_data/ocel/*.jsonocel`, `data/*.xes`

### From `ex4pm` (Elixir semantics donor — port algorithms, not code)

- `lib/ex4pm/ocel.ex` (IR + normalize + validate), `ocel_writer.ex`
- `lib/ex4pm_engine/decomposition/wf_net_to_powl.ex` (Alg. 1–3), `powl.ex`, `powl/choice_graph.ex`, `powl/language.ex`, `powl/shuffle.ex`
- `lib/ex4pm_engine/workflow_net.ex`, `ocpn.ex`
- `lib/ex4pm_engine/alignment.ex` (incl. `align_ocpn/3`), `engine/conformance/token_replay.ex`, `etc_precision.ex`
- Fixtures: `test/fixtures/sepsis.xes`, `domestic_declarations.xes`, `marketplace-ocel.json`, `ocel/self_conformance.ndjson`

### From `beam4pm` (fixtures + bridge patterns)

- `lib/beam4pm_art72_conformance.ex` (engine-free replay convention + fitness formula)
- `lib/beam4pm_hddl_powl.ex`, `beam4pm_fond_powl.ex`, `beam4pm_replan_router.ex`, `beam4pm_powl_conformance.ex` (observe-replan), `beam4pm_zcode_ocel_verifier.ex`
- Fixtures: `qualification/fixtures/*.xes` (incl. bpi2020 50-trace sets), `test/fixtures/zcode_ocel/*.jsonocel` + receipts, `ocel_session_facts/*`, `dfcm/{dfcm.hddl, dfcm-fond.pddl}`

### From `ash_ex4pm` (Ash membrane patterns — reference for a future purrdf-process binding layer)

- `lib/ash_ex4pm/dsl.ex` (activity/object_type/object_relationship entities), `notifier.ex` (envelope mint, bindings, sha256 ids), `changes/brce_gate.ex`, `changes/receipted_action.ex`, `errors/refused.ex`
- Negative finding: no tenant binding anywhere — object-centric (actor + pk + relationships) only

### From `ash_pplan` (planning contract)

- `lib/ash_pplan/fond.ex` + `fond/synthesis|policy_switch|differential|tla.ex`, `sa2a/refusal.ex`, `sa2a/policy_candidate.ex`, `fond/counterexample.ex`, `execution_receipt.ex`, `standing/ladder.ex`, `process_evidence.ex`, `reactor/durable/ledger_ocel.ex`
- Fixtures: `test/fixtures/fond/tla/*.json` (12 court fixtures, schema `ash_pplan/fond-tla-fixture/v1`), `wave_01.json`, `wave_03.json`; `planning/**/*.hddl`; `test/support/marketplace_sim/lifecycle_plan.ttl`

### From `bcinr` (silicon-grade kernels)

Already in `purrdf/crates/branchless` (17 modules, ported from bcinr @ `1e92b213`): bitset slice ops, SWAR, Eytzinger, wyhash, sort networks, packing/morton-2d, filters, circuit-breaker masks.

**Gaps to port as new branchless jobs** (per helpers-ledger `entry_points`, no second bodies):

| Kernel | bcinr source | purrdf-process use |
|---|---|---|
| Pred-mask readiness gating (`pred_mask & !done_mask == 0`) | `bcinr-powl/src/scheduler_v2.rs:38-43` | HTN precondition gating, Petri transition enabling |
| `bit_parallel_sort8_u32(val, aux)` | `algorithms/bit_parallel_sort8_u32.rs:25` | packed event-key ordering |
| `shuffle_fisher_yates_branchless` | `algorithms/shuffle_fisher_yates_branchless.rs:28` | trace randomization for differential conformance |
| `bloom_filter_graph_visited` | `algorithms/bloom_filter_graph_visited.rs:22` | HTN visited-set dedup |
| Branchless transitive closure (bitset-row Floyd–Warshall) | replace `bcinr-powl/src/process_toolkit.rs:280-294` branchy form; oracle `wf_to_powl.rs:504` | partial-order shuffle evaluation, reachability |
| `morton_encode_3d_u32` | `algorithms/morton_encode_3d_u32.rs:22` | spatial retrieval joins (only if geo-fused) |

bcinr test style to replicate: reference oracle + 3 mutants + proptest per kernel, inline
`#[cfg(all(test, not(miri)))]`; differential oracles vs vendored AGPL POWL reference
(`oracles/powl/cases.json`). Bench harness is Divan; measured baselines in
`docs/benchmarks/BASELINE.md` (M3 Max; `bitset_intersect_64_words` 27.05 ns,
`conformance_check_pass` 0.41–1.03 ns — the class of numbers `purrdf-process` courts).
Note: `sort8_8u32` and `nonzero_byte_mask_word` do not exist under those names — the real
carriers are `optimal_sort_8_u32` (19 comparators, depth 6) / `bitonic_sort_8u32` (ported)
and `swar_str.rs` byte-lane masks; bcinr's inline criterion benches are orphaned (no
`[[bench]]` target registers them — audited by `tools/bcinr-bench-auditor`).

### purrdf constraints the new crate inherits (evidence)

- `layers.toml` row required; sit above `purrdf-branchless` (`layers.toml:399-402` shows the pattern; pending rows at :306/:322/:337/:492/:508)
- `helpers-ledger.toml`: consume `purrdf-branchless` kernels by path, extend `entry_points`, never add second bodies
- No Cargo features; `purrdf_core::FastMap`/`FastSet` only (std `RandomState` clippy-banned, `AGENTS.md:353-358`); wasm32-clean (no threads/fs/wall-clock-RNG on the wasm path)
- `crates/purrdf-process` does not exist yet (confirmed); `crates/datalog/src/demand.rs` (magic-set demand transformation, `evaluate_demanded` :231) is the sibling to study for proof-carrying conformance evaluation

## 4. Answers to the standing inventory questions

1. **Does wasm4pm already contain the core OCEL 2.0 parser?** Yes —
   `wasm4pm/src/models.rs:762-880` + `ocel_io.rs` + `ocel_v2.rs`, dual-dialect
   (1.0 `object_ids` / 2.0 `object_refs`), with validation, lifecycles, flattening, and
   NDJSON streaming. It is the extraction source; ex4pm contributes only the
   attribute-time-series nuance and the byte-identity writer discipline.
2. **Do bcinr's bitset primitives match the planned `purrdf-process` memory layout?**
   Yes for the substrate (u64-slice bitsets + masks + SWAR already ported to
   `purrdf-branchless`), but the *process-shaped* kernels (pred-mask readiness, bit-parallel
   reachability/shuffle validation, visited-set dedup) live in `bcinr-powl`/`bcinr-pddl`
   and are NOT yet ported — they are the delta, and the natural
   `conformance/`+`synth/` fast paths.
