# [OPT-57] Route LEB128 and Base64 Chunk Kernels to Existing Homes

## 1. TARGET CONTEXT
- **Crate & File:** homes already exist: `crates/rdf-core/src/ir/pack/bits.rs:1052`
  (`write_varint`, LEB128 encode; decode at `:1066` region, doc at `:1065`) and
  `purrdf_xsd::parse_base64` (called from `crates/mime/src/transfer.rs:74`)
- **Subsystem:** Ingestion (LEB128) | Egress (base64 transfer decode)
- **Inspection Agent:** OPT-42 ticket-writer lane (bcinr handoff §3.5)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** bcinr ships standalone `leb128_encode_u64`
  (`~/bcinr/crates/bcinr-logic/src/algorithms/leb128_encode_u64.rs:29`),
  `leb128_decode_u64` (`leb128_decode_u64.rs`), and chunked base64 kernels —
  `base64_decode_chunk4` (4-byte chunk, branchless `decode_sextet`,
  `base64_decode_chunk4.rs:28-46`), plus `base64_{encode,decode}_simd`. PurRDF
  already owns both jobs:
  - LEB128: `purrdf_rdf_core::ir::pack::bits::write_varint` +
    `read_varint` (`bits.rs:1050-1090`), consumed by
    `crates/rdf-core/src/artifact/identity.rs:32` (explicitly documented as
    reusing pack/bits rather than restating).
  - base64: `purrdf_xsd::parse_base64`, the single sanctioned decode home, called
    by mime transfer decode (`crates/mime/src/transfer.rs:74`) and validated in
    the NMTOKEN-class path (`transfer.rs:309`).
- **Hardware-Level Impact:** none to fix — this ticket forbids a port. A second
  LEB128 or base64 body would be an O(N^2) drift surface
  (N-implementations-of-one-calculus).
- **Baseline Cost:** n/a — no perf change claimed.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** ROUTE, DO NOT PORT.
  - Any future varint/LEB128 need anywhere in the workspace calls
    `purrdf_rdf_core::ir::pack::bits::{write_varint, read_varint}` — the
    home-per-job. bcinr's `leb128_{encode,decode}_u64` are reference oracles for
    differential tests only.
  - Any base64 chunked-decode need calls `purrdf_xsd::parse_base64`. If a
    measured ingest hot path needs chunked throughput, the extension happens
    INSIDE the xsd home (chunked kernel behind the same `parse_base64` signature,
    `crates/xsd` lexical layer — which is where purrdf_lex/xsd readers live), never
    as a sibling body in mime or branchless. bcinr's `base64_decode_chunk4` /
    `decode_sextet` branchless sextet table is the reference form for that
    possible in-home extension.
- **Hard Bounds Applied:** one home per job (`helpers-ledger.toml`); no new module,
  no `branchless`-crate base64, no mime-local base64 kernel; any in-home chunked
  extension keeps the exact `parse_base64(&str)` signature and error taxonomy.
- **Proposed Type Layout & Signatures:** unchanged —
  `pub fn write_varint(out: &mut impl Extend<u8>, value: u64)` and
  `pub fn read_varint(...)` in `purrdf_rdf_core::ir::pack::bits`;
  `purrdf_xsd::parse_base64(&str) -> Result<...>` — signatures frozen.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Allocation Invariant:** no change; existing homes keep their alloc profile.
- **Home-check receipt:** verified on disk 2026-10-26 — `helpers-ledger.toml` grep
  for base64/leb128 homes returns the xsd/rdf-core path (no branchless/mime row);
  `crates/mime/src/transfer.rs:74` and `crates/rdf-core/src/ir/pack/bits.rs:1052`
  are the live call/def sites. Any PR adding a second body fails
  `check-shared-helpers.py`.
- **Over-refusal neighbor:** if the xsd home ever gains the chunked kernel, the
  court must run the canonical mime transfer decode corpus (valid padded,
  unpadded, and whitespace-adjacent inputs — `transfer.rs:76` "malformed base64
  groups or padding") both directions: previously-accepted inputs stay accepted
  (over-refusal guard), the invalid corpus stays refused.
- **Benchmark Target:** none required (no perf change claimed); if the in-home
  chunked extension is ever executed, a base64 decode bench in
  `purrdf_testkit::bench` is a precondition per AGENTS.md.
- **Layer Boundary:** `check-layers.py` clean; no ledger mutation needed (homes
  already sanctioned).

Status note: LEB128 home is landed and wired (rdf-core pack/bits, consumed by
artifact identity). Base64 home is landed (`purrdf_xsd::parse_base64`) and wired
via mime transfer. bcinr kernels remain landless-but-written upstream and stay
there as oracles.
