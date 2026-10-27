# [OPT-01] Route Scalar Delimiter Scans Through Existing SIMD Kernels

## 1. TARGET CONTEXT
- **Crate & File:** `crates/lex/src/json_escape.rs:505,529-532`; `crates/lex/src/terminals.rs:1514,1531-1545`; `crates/lex/src/percent.rs:402,409-430`; `crates/lex/src/xml/reader/dtd.rs:311-315,343-346,779-800`; `crates/rdf-core/src/cdt_blank.rs:970-1010,1023-1033`
- **Subsystem:** Ingestion
- **Inspection Agent:** Agent 01 (SWAR Delimiter Scanner)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** e.g. `let Some(first) = bytes.iter().position(|&b| b == b'\\') else { ... }` (json_escape.rs:505); `while at < bytes.len() { if bytes[at] == b'\\' ... else at += 1 }` (terminals.rs:1531-1545, percent.rs:409-430); two-needle DTD scan `position(|&b| b == b'<' || b == b'&')` (dtd.rs:779-800); CDAT state machine `while i < bytes.len() { match bytes[i] { b'<' ... b'"' | b'\'' ... } }` (cdt_blank.rs:970-1010).
- **Hardware-Level Impact:** 1–2 compare+branch per byte over every JSON string body, UCHAR sequence, IRI normalization, and CDAT region; no ILP across bytes; CDAT's multi-arm match has the worst misprediction profile (all arms plausible).
- **Baseline Cost:** 1 branch/byte serialized; SIMD kernels process 16–64 bytes/iteration.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** No new code on the preferred path — route every site to the existing `purrdf_lex::scan` kernels: `find_byte` (1 runtime needle), `find_needles::<N>` (2–4 runtime needles, 64-byte block walk; lowered to `pcmpeqb`/`pmovmskb`, aarch64 `cmeq+addp`, wasm `i8x16`). Multi-needle sites: `find_needles::<2>(.., [b'<', b'&'])` (DTD), `find_needles::<4>(.., [b'<', b'"', b'\'', b'_'])` (CDAT pre-filter), `find_needles::<2>(.., [b'>', b'\\'])` for `scan_iriref`. If strict u64-SWAR is mandated, wire the dead `rdf-core::primitives::swar_find_delimiter_u64` (primitives.rs:9) behind a chunk wrapper: `while i + 8 <= len { mask = swar_find_delimiter_u64(load8, d); ... i += 8 }`, scalar tail `< 8` bytes. Escape-pair skipping (`i += 2`) stays scalar at hit sites.
- **Hard Bounds Applied:** chunk = exactly 8 bytes (u64 SWAR form) or 16-byte kernel block; scalar tails always; no overread (`u64::from_le_bytes` on len-8 slice).
- **Proposed Type Layout & Signatures:** reuse existing: `ByteClass::find_first`, `find_byte(bytes, needle)`, `find_needles::<N>(bytes, [u8; N])` (`crates/lex/src/scan.rs:389-519`). Optional new home beside them: `find_byte_rev` for markdown `rposition` (`crates/markdown/src/split.rs:46,76`).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** no per-byte `position` loops remain in the six hot sites; hit-chunk dispatch and tails stay scalar (once per token boundary, not per byte).
- **Allocation Invariant:** 0 heap allocations (pure scan).
- **Benchmark Target:** ≥4× throughput on escape-heavy JSON strings vs. scalar baseline; add to `primitives` bench suite (see OPT-20).
- **Layer Boundary:** kernels stay in `purrdf-lex` / `purrdf-core` per `layers.toml`; zero violations via `check-layers.py`.
- **Implemented in `purrdf::branchless::{swar, structural}`** (crate `crates/branchless`, ported from
  BranchlessCInRust): the Mycroft word/slice kernels and JSON escape/structural mask primitives
  land as the shared SWAR home these sites route through.
- **Amendment (scan handoff §3.4) — canon as an OPT-01 consumer:** the RDFC-1.0
  canonicalization render path is a per-quad byte-scan consumer of this surface:
  `hash_first_degree` (`crates/rdf-core/src/ir/canon.rs:2462-2467`) → `write_component`
  (`canon.rs:2793`) → `write_slot` (`canon.rs:2813`) → `write_iri`
  (`crates/lex/src/term_syntax.rs:56-61`) → `iri_escape::push_escaped`
  (`crates/lex/src/iri_escape.rs:239`) → `find_first_candidate`
  (`iri_escape.rs:162-166`) → `ByteClass::find_first`
  (`crates/lex/src/scan.rs:318`). The scan kernel itself is already landed — canon needs
  no new kernel, only (a) confirmation that `hash_first_degree`'s scratch-buffer rewrite
  (OPT-09 step 1) keeps routing through `find_first_candidate` rather than regrowing a
  per-byte loop, and (b) OPT-09 step 7's nested-triple suffix sites ride the same kernel.
  **Correction to earlier claim:** `branchless::simd_fallback` (`crates/branchless/src/
  simd_fallback.rs:41-324`) carries only word-level ct_/pdep/pext/shuffle/blend ops —
  there is no byte-scan surface there. The OPT-01 scan surface for canon (and all
  consumers) is `branchless::swar` (`find_byte_mask` swar.rs:225,
  `swar_find_all_positions` swar.rs:242, `count_leading_eq_u8` swar.rs:271) and
  `branchless::structural` (`escape_mask` structural.rs:70, `structural_mask`
  structural.rs:99); `simd_fallback` is the constant-time op home, not a scan home.
