# [OPT-02] Token64 Stack Buffer + Borrowed Token Path Through the Interner

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf/src/native_codecs/text_parse.rs:163-178,2265,302-311,1996-1998`; `crates/n3/src/lexer.rs:4-50`; `crates/rdf/src/native_codecs/hextuples.rs:214-285`; `crates/xsd/src/value.rs:219,322-325`, `decimal_float.rs:119,258,264`, `temporal.rs:261,4784,4840,4877,4915`; `crates/rdf-core/src/iri/resolve.rs:70` (via `crates/iri/src/resolve.rs:70`)
- **Subsystem:** Ingestion
- **Inspection Agent:** Agent 02 (Token Truncation & Scalar Interning)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `enum Node { Iri(Iri), Bnode(String), Literal { value: String, ... }, Triple(Box<Self>, Box<Self>, Box<Self>) }` (text_parse.rs:163-178) — 3–7 heap Strings per statement; `let expanded = format!("{namespace}{local}");` per prefixed name (text_parse.rs:2265); `pub enum TokenKind { Iri(String), PName(String), ... }` with `lex(input) -> Result<Vec<Token>>` (n3/lexer.rs:4-50) owning every token; `"true"/"false".to_string()` per boolean (value.rs:219).
- **Hardware-Level Impact:** allocator lock + cache pollution per token; every term materializes heap strings before the interner's borrowed-key lookup (`TermLookup<'a>`, builder.rs:250) can consume them.
- **Baseline Cost:** 3–7 allocations per N-Triples statement; 1 per token in N3; 1–2 per XSD canonical scalar.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (a) New `Token64` stack buffer in `purrdf-lex` (beside `small.rs` conventions) + `impl TextOut for Token64` in `text_out.rs` so XSD boolean/numeric/temporal canonicalizers write to the stack. (b) `TermLookup` gains an `IriPrehashed { text: &'a str, hash: u64 }` arm backed by `purrdf_hash::fixed` so >64-byte tokens are hashed in place, compared borrowed-vs-arena only on probe hit — never materialized. (c) De-own ingestion: `Node<'a>` with `Cow<'a, str>` fields mirroring the proven `sparql-algebra::Token<'a>` pattern (lexer.rs:171-174 doc); `subject_key` becomes a borrowed hash key. (d) `cdt_embedded_blanks` probe (`contains_embedded_blank(lexical, datatype) -> bool`, pure scan, home `purrdf_core::cdt_blank`) before allocating the per-miss Vec (builder.rs:1108-1128). (e) Ledger compliance: record `Token64` under a new helpers-ledger job, home `purrdf_lex`; reuse `iri-reference-resolution` row.
- **Hard Bounds Applied:** token ≤ 64 bytes → `Token64 { len: u8, bytes: [u8; 64] }`; oversize → 64-bit prehash path immediately.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct Token64 { len: u8, bytes: [u8; 64] }             // stack, no heap
  impl Token64 { pub fn from_slice(s: &[u8]) -> Option<Self>; pub fn as_slice(&self) -> &[u8]; }
  pub enum TokenRef<'a> { Inline(Token64), Borrowed(&'a [u8]) }
  ```

## 4. VERIFICATION INVariant & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation-focused); the ≤64 B/oversize split is a single predictable compare.
- **Allocation Invariant:** 0 heap allocations for statements whose every token ≤ 64 B; XSD canonical forms never allocate.
- **Benchmark Target:** N-Triples/Turtle ingest allocation count per 10k statements drops from ~50k to ~0; bench in the OPT-20 suite.
- **Layer Boundary:** `Token64` lives in purrdf-lex only; consumers reference it, never re-implement (`check-shared-helpers.py` clean for the new job).
- **Amendment (scan handoff §3.4):** token *validation* (charset/escape checks on
  `Token64::from_slice` input and the `TokenRef` borrowed path) routes through the landed
  `purrdf::branchless` kernels — no second implementation in purrdf-lex. Exact calls:
  - `branchless::swar::classify_bytes(word: u64) -> u64`
    (`crates/branchless/src/swar.rs:110`) — per-lane digit/alpha/whitespace/upper/lower
    bitfield, the per-word token charset check for 8-byte token words.
  - `branchless::swar::byte_lane_in_range(word: u64, lo: u8, hi: u8) -> u64`
    (`crates/branchless/src/swar.rs:28`) — range-restricted charset masks (e.g. PN_CHARS
    sub-ranges) without per-byte branches.
  - `branchless::swar::parse_4_hex_digits(word: u32) -> Option<u32>`
    (`crates/branchless/src/swar.rs:155`) — `\uXXXX` UCHAR validation inside tokens.
  - `branchless::char_class::parse_hex_u32(bytes: &[u8]) -> Result<u32, ()>`
    (`crates/branchless/src/char_class.rs:116`) and
    `char_class::parse_decimal_u64(bytes: &[u8]) -> Result<u64, ()>`
    (`crates/branchless/src/char_class.rs:140`) — XSD scalar lexing feeding the Token64
    fast path (integer/decimal/hex forms).
  - `branchless::swar::is_all_ascii(bytes: &[u8]) -> bool`
    (`crates/branchless/src/swar.rs:296`) — the ≤64 B fast-path admission gate; non-ASCII
    tokens fall to the existing scalar/char-boundary path.
  - `branchless::swar::find_first_byte_in_slice(bytes: &[u8], target: u8) -> Option<usize>`
    (`crates/branchless/src/swar.rs:346`) / `find_byte_mask(bytes, target) -> u64`
    (`crates/branchless/src/swar.rs:225`) — delimiter location inside the token buffer.
  Dependency direction: `purrdf-branchless` is a purrdf-hash-only leaf
  (`layers.toml:386-391`); wiring it under `purrdf-lex` requires one new `layers.toml` edge
  (`purrdf-lex` → `purrdf-branchless`), to be verified with
  `python3 scripts/check-layers.py` after the edge lands.
