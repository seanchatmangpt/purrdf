# [OPT-17] Wire ArrowEgressHeader Into Production Egress; Consolidate Escape Law

## 1. TARGET CONTEXT
- **Crate & File:** `crates/arrow/src/header.rs:12-23` (header, already built); gap sites `crates/sparql-results/src/json.rs:101-106`, `crates/rdf-capi/src/query.rs:739`, `crates/rdf-wasm/src/query.rs:2327-2359`, `crates/n3/src/printing.rs:603-640`, `crates/gts/src/mmr.rs:590-640`
- **Subsystem:** Egress
- **Inspection Agent:** Agent 17 (128-Byte Canonical Arrow IPC Framing)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - The framing already exists: `ArrowEgressHeader`, `#[repr(C, align(64))]`, `SIZE = 128`, magic `b"GLAWARRW"`, `SCHEMA_VERSION = 0x0002`, `PAYLOAD_OFFSET = 128`, with two BLAKE3 witness digests — `triples_digest` `[36..68]`, `columnar_digest` `[68..100]` — reserved region `[100..128]` zero-checked, and determinism flags `FLAG_HAS_NULLS` / `FLAG_BIG_ENDIAN` / `FLAG_STRICT_PROJECTION_ORDER` with undefined bits rejected; `canon_arrow_empty_v1()` (header.rs:176-179) gives a fixed domain digest for non-tabular results (ASK/empty), so the envelope is total over `SparqlResult`'s three shapes.
  - **Gap:** the only consumer is a test (`crates/duckdb/tests/chicago_sql_edge_cases_court.rs:559-628`); no production writer binds a payload slab, and `crates/columnar` (`writer.rs`, `files.rs`) shares nothing with `purrdf-arrow`.
  - Eager document-sized encoding on hot egress: `let mut out = String::with_capacity(output_size_hint(result)); write_srj(..., &mut out)?;` → `out.into_bytes()` (`json.rs:101-106`); `let outcome = purrdf_sparql_results::to_json(&result, ...)` held as a whole `Vec<u8>` crossing FFI (`rdf-capi/src/query.rs:739`); whole JS string per query (`rdf-wasm/src/query.rs:2327-2359`).
  - Duplicate escape law outside the `purrdf_lex::json_escape` home: local `escape_json` + per-term `format!`/`Vec<String>` in `n3/src/printing.rs:603-640` (`named_node_json`, `rdf12_json`), and a third hand-rolled escaper in `gts/src/mmr.rs:590-640` (`Proof::to_json`).
- **Hardware-Level Impact:** document-sized `String`/`Vec<u8>` residency per query result (allocator churn, L2/L3 pressure, TLB churn) on FFI/JS boundaries; three divergent escape implementations = drift risk on a byte-determinism-critical path.
- **Baseline Cost:** O(document) allocation per result; eager arm's cost includes growing and handing back a document-sized `String` (acknowledged at `sparql-results/benches/graph_serialize.rs:166`).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. Reuse `purrdf_arrow::ArrowEgressHeader` verbatim — no design change; only wiring.
  2. Payload: after the 128-byte header, one or more zero-copy record-batch slabs at `payload_offset = 128` (itself 64-aligned); each batch contributes to `columnar_digest` (BLAKE3 streamed over the payload region); `batch_count` = slab count; `triples_digest` = BLAKE3 over the source triple witness (both sides already demonstrated at `chicago_sql_edge_cases_court.rs:602-628`); 64-byte trailing padding to `total_byte_len`.
  3. Egress routing (one home per job): CLI/duckdb keep the existing streaming law — `serialize_into` + `TextSink` over the 64 KiB staging window (`DRAIN_BUFFER_BYTES`, `crates/rdf-core/src/sink.rs:97`) — for human formats; a new `canon-arrow` results-format arm emits `header.to_bytes()` followed by slabs written straight from Arrow buffers via `OutTarget`; no intermediate `String`/`Vec<u8>` document is ever formed. `rdf-capi`/`rdf-wasm` either migrate to that streaming lane or receive framed binary — `(header_ptr, slab_ptrs)` — instead of a monolithic UTF-8 string.
  4. SRJ text emission keeps its single escape home (`purrdf_lex::json_escape`, including the streaming `JsonEscaping<W: TextOut>` adapter, `json.rs:302-327`); consolidate the `n3` and `gts` escapers into that law.
- **Hard Bounds Applied:** header exactly 128 bytes, 64-byte aligned; payload offset fixed at 128; trailing padding exactly 64 bytes; `total_byte_len == payload_byte_len + 128 + 64` validated on decode (header.rs:104-108).
- **Proposed Type Layout & Signatures:** unchanged header layout (fixed field order = wire order: magic `[0..8]`, version `[8..10]`, flags `[10..12]`, `batch_count` `[12..16]`, `payload_offset` `[16..20]`, `payload_byte_len` `[20..28]`, `total_byte_len` `[28..36]`, digests `[36..100]`, reserved `[100..128]`). Slab layout defined once, referencing `purrdf-arrow` (avoid a second framing authority over `crates/columnar`'s independent five-table container).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (framing/alloc ticket); fixed-offset encode/decode is branch-free field copies with a single flags-mask check.
- **Allocation Invariant:** canon-arrow egress performs 0 document-sized heap allocations — header `to_bytes()` returns `[u8; 128]`, slabs are written from existing Arrow buffers.
- **Benchmark Target:** FFI/JS result egress drops from O(document) allocation to O(slabs); add to the bench suite per OPT-20.
- **Layer Boundary:** header/escaper consolidation verified via `check-layers.py` (270 edges stay clean); golden-byte coverage for a production writer following the `crates/columnar/tests/golden_bytes.rs` house pattern; byte-determinism compliance (no timestamps/env in the envelope, per AGENTS.md:84 and AGENTS.md:344).


---

## Status (2026-10-27, wiring-wave sync lane; disk-read, not runner-cited)

- **Item 1 — `canon-arrow` results-format arm (`crates/sparql-results`): DONE.**
  Format surface + writer both landed (commit 6e93aca3): `CanonArrow` variant
  (`src/lib.rs:105`), member of `ALL` (`src/lib.rs:121`), MIME token
  `application/x-purrdf-canon-arrow` (`src/lib.rs:140-141`), the eager +
  streaming dispatch arms (`src/lib.rs:246-255`, `:301-303`), and the writer
  `src/canon_arrow.rs` (627 lines: three-pass slabbed emitter, zero
  document-sized allocations in the streaming lane, canon-empty frame for
  non-tabular shapes, in-file Chicago tests). Courts: streaming-bounded
  allocator oracle, golden bytes, refusals-before-emission, structured
  errors; bench arms in `benches/graph_serialize.rs` (UNRUN — receipts via
  phase-5 fast lane or CI).
- **Item 2 — FFI/JS seams (`rdf-capi`, `rdf-wasm`): DONE.**
  `purrdf_query_canon_arrow` (`crates/rdf-capi/src/query.rs:778`, header
  `purrdf.h:2885`, ABI snapshot line 72, 3 tests); wasm `queryCanonArrow`
  (`crates/rdf-wasm/src/query.rs:1588`, `JobOutcome::Raw(Vec<u8>)` byte lane,
  `js/index.mjs` + `index.d.ts` exposure, round-trip court). The monolithic
  text egress remains as the compatibility lane, unchanged.
- **Item 3 — escaper consolidation: DONE.** `gts` delegates to
  `purrdf_lex::json_escape` (`crates/gts/src/mmr.rs:17,585`); `n3`'s
  hand-rolled `escape_json` is deleted and `JsonEscapes::Minimal` wired at
  `printing.rs:741/:748/:771/:773`, with the golden pin at :796-825. The
  public byte change (uppercase → lowercase `\uXXXX`; `\b`/`\f` short
  forms dropped) is court-verified SAFE: no frozen fixture carries the old
  spelling, and the only consumer is the new in-file golden.

Addendum enrichment items (`BadMagic([u8; 8])`, `ReservedNotZero{offset,
value}`) remain OPEN — not applied.

---

## Addendum (2026-10-27, arrow 20-lane review): error-payload enrichment

Two refusals are under-specified for diagnosis and belong in this ticket's
scope (found by the arrow adversarial/error-taxonomy review; not yet
applied — variant-payload changes are semver-visible and were deferred):

- `BadMagic` (crates/arrow/src/header.rs): carries no observed bytes, so a
  typo (`GLAWARROW`) is indistinguishable from total corruption. Add the
  observed 8 bytes (e.g. `BadMagic([u8; 8])`) or a Display-side hexdump.
- `ReservedNotZero`: names no offset. Add the first offending byte index
  within 100..128 (e.g. `ReservedNotZero { offset: usize, value: u8 }`).

Both require updating the Display + `error_visit!` arms and every test
asserting these variants (crates/arrow/tests/arrow_header_test.rs). Note
for implementers: OPT-66/67/68 are NOT arrow tickets — they are the
renumbered batch-count/branchless/gts-cross-check tickets; enrichment
lives here with the rest of the arrow scope.

Plan-of-record update (2026-10-27, post-landing): the escaper
consolidation is DONE (see Item 3); the n3 golden pin lives at
crates/n3/src/printing.rs:785 (the golden module, not the deleted
escaper). Remaining open scope in this ticket: the error-payload
enrichment items above, and the framed-lane production consumers
beyond capi/wasm (rdf-capi CLI surfaces) as adoption proceeds.
