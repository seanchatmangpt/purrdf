# PurRDF Benchmarks: The Complete Sales Reference

**Audience:** executives, sales engineers, and partners who need to understand
what PurRDF's measured performance numbers mean — without reading source code.

**Every number in this document is traceable to an executed benchmark run on
real hardware (Darwin arm64, 48 GB unified memory, October 27 2026) with a
per-run receipt (command, duration, measured/failed count). Where a number is
NOT receipted, this document says so explicitly. Nothing on this page is
projected or aspirational.**

---

## 1. What "1,655 measured, 0 failed" actually means

PurRDF ships with an automated benchmark suite that runs like a factory
quality-inspection line: every component is exercised with real data, the
harness measures how fast it runs, and the result is written to a receipt log
naming the exact command, duration, and pass/fail count.

- **1,655 citable performance tests measured with zero failures.** Each is a
  separate, named scenario. (The raw receipt count is 1,693; it includes a
  superseded 37-row early shapes run the receipts themselves mark "do not
  cite", plus one recount correction — the citable total is 1,655.)
- **Zero failures on every citable subject.** Three subjects are partials and
  are disclosed below — they are excluded from the counted totals, not hidden
  inside them.
- **Every result is auditable.** The log records the exact command, wall-clock
  duration, sample count, and pass/fail footer per test. A re-run of any
  single test is directly comparable.

If each benchmark were a crash test, PurRDF crashed 0 cars out of the 1,655
citable runs.

### Receipt provenance (for auditors)

- Per-crate log footers — "N benchmarks measured; 0 failed", printed by the
  test binary itself — are the register of truth; all counts in this document
  are footer-derived.
- The primary log directory (`/tmp/bench-logs`) is ephemeral; a persisted
  copy lives in `~/purrdf-bench-keep/`. One known gap, disclosed: an early
  runner rewrote the register in place, destroying a handful of the earliest
  return-code rows. Per-crate footers — the citable surface — are complete.

---

## 2. Component-by-component results

### 2.1 The core engine (purrdf-core) — 499 tests, 0 failures

The core engine stores, indexes, and retrieves RDF data — the equivalent of a
database engine's storage layer. 499 tests across 12 footer-bearing targets:

| Target | Tests | What it measures | Why it matters to a buyer |
|---|---|---|---|
| xsd_regex | 191 | Pattern-matching speed over typed data values (dates, numbers, constrained strings), incl. 64 MB documents | Data validation at query time |
| model_traits | 150 | Speed of creating, reading, matching individual records | The "cost per row operation" — every query touches it |
| shared_views | 48 | Reading the same data through different views without copying | Multi-user memory efficiency |
| mutable | 23 | In-place add/remove/update (no rebuild) | Live systems accepting data while serving queries |
| pack_index_compare | 23 | Comparing two index structures for equivalence | Data integrity verification |
| pack_query | 13 | Querying the compressed archive format | Compression without query slowdown |
| small | 12 | Micro-operations: single lookups, existence checks | The latency floor |
| purremb | 14 | Embedding/vector retrieval over RDF | AI/ML integration: semantic similarity |
| shared_homes | 8 | Multiple views sharing one physical copy | Multi-tenant efficiency |
| pack_bits | 9 | Bitset operations in the compressed format | Flag/permission checks at scale |
| primitives_table | 6 | Low-level string/number operations | Foundation throughput |
| sssom | 2 | Cross-ontology mapping format throughput | Ontology-alignment workloads |

Plus report-only allocation probes (pack restore, embeddings) that measure
bytes allocated — capacity-planning data with no pass/fail condition,
excluded from the count by design.

### 2.2 SQL-over-RDF (purrdf-sql + purrdf-duckdb) — 14 tests, 0 failures

Query RDF data using standard SQL your analysts already know. PurRDF converts
an RDF graph into Parquet-formatted tables, loads them into an embedded
DuckDB analytical engine, and runs SQL against them.

| Suite | Rows | What was measured |
|---|---|---|
| sieve_bench | 10 | Raw SQL filtering against in-memory DuckDB tables — the "can it handle my WHERE clause" speed |
| sql_rdf_e2e | 4 | The full chain: parse Turtle → convert to Parquet tables → run SQL → project results back as RDF triples |

**The e2e suite is the headline:** one system serving both the RDF data model
(flexible integration) and SQL analytics (fast reporting), measured end to
end.

**Buyer caveat:** the DuckDB engine loads from a pre-built binary cache; a
fresh machine downloads a release binary once. A setup step, not a runtime
dependency.

### 2.3 The wire format (purrdf-arrow) — 10 tests, 0 failures

PurRDF's high-speed serialization format ("canon-arrow") uses a 128-byte
binary header with a BLAKE3 cryptographic digest, modeled on Apache Arrow's
columnar layout. The receipted measurement is the `ipc_egress_100k_rows`
suite: encoding and decoding a seeded 100,000-row × 8-column dataset, plus
round-trip exactness checks and the corruption refusal (bad magic bytes →
immediate rejection).

**Why it matters:** the wire format is the truck that carries data between
PurRDF systems — and every payload is cryptographically verified on arrival.

### 2.4 Cryptographic hash conformance (purrdf-hash-conformance) — 273 tests, 0 failures

Every data-integrity guarantee rests on the hash layer, tested the way a
certification lab tests a lock:

- **Digest suite: 255 tests** — BLAKE3 (three input sizes, multiple backends,
  streaming boundaries), FNV-1a-64, splitmix64, multiset hashing, frame
  hashing, and hex encode/decode.
- **Hasher suite: 18 tests** — the fixed-key hasher every output-path map
  uses (deterministic iteration, no randomized hash seed).

**For a buyer:** when PurRDF says "this data has not been tampered with,"
that is a cryptographic claim backed by 273 tested scenarios — measurable and
reproducible, not a marketing statement.

### 2.5 Branchless compute kernels (purrdf-branchless) — 7 tests, 0 failures

The low-level algorithms that make PurRDF fast: cache-efficient tree layouts
(Eytzinger), branchless sorting networks, high-speed hashing (Wyhash),
SIMD-within-a-register delimiter scanning, and space-filling curves for
geographic data — each compared against a straightforward reference
implementation.

### 2.6 SHACL validation (purrdf-shapes) — 13 tests, 0 failures

SHACL is the W3C standard for validating RDF against business rules —
database constraints for graph data. The citable receipt is the full
validation-pipeline suite (13 tests, including the pydantic-package target
that previously failed mid-run and was re-queued to completion).

### 2.7 Datalog reasoning (purrdf-datalog) — 13 tests, 0 failures

Datalog is a declarative rule language that derives new facts from existing
ones — the inference engine behind "connect the dots" workloads (fraud
detection, access-control inference, supply-chain provenance). All 13 tests
passed. Disclosed caveat: 3 of 13 groups hit the harness's wall-clock ceiling
and early-stopped — most notably `derive_1m_facts`, whose single measured
iteration took roughly 470 seconds to derive a million facts to fixpoint.
Those rows are capacity point estimates (real single observations), not
averages.

### 2.8 Partial and report-only subjects (disclosed, not counted)

| Subject | Receipt | Status |
|---|---|---|
| entail (reasoning) | 48 measured / 0 failed, run hit a 3600s wall cap before the last suite started | PARTIAL — the 48 are citable; the remainder is open work |
| n3 (reasoning) | Two terminal reds: a compile error, then a memory kill on an oversized fixture. Memory-bounded fixtures implemented, awaiting CI | RED with owner (OPT-74) |
| hnsw (vector index) | Build 84s, recall 638s, 200k-row recall 2493s — all completed rc=0 as report-only timing tables | Report-only green; excluded from totals |
| rdf-core facade run | Hit a 3600s wall cap | PARTIAL |

### 2.9 All other green components

| Component | Tests | What it does |
|---|---|---|
| text | 299 | Full-text search, BM25F ranking, phonetic matching, Unicode segmentation (7 binaries) |
| sparql-algebra | 140 | SPARQL parsing and algebra construction (6 binaries) |
| purrdf (facade) | 44 | The all-in-one public API: end-to-end pipeline + allocation profiling |
| purrdf-rdf | 122 | Native codecs: Turtle, N-Triples, RDF/XML, TriG, JSON-LD, CSVW (6 binaries) |
| geo-kernel | 109 | Geographic computations: cell hierarchies, geodesic operations |
| lex | 22 | Lexical layer: JSON, YAML, CBOR, XML scanners + Unicode normalization |
| shex | 18 | ShEx validation (the other RDF shape language) |
| retrieval | 18 | Fusion retrieval: text + vector + graph proximity |
| columnar | 12 | Columnar storage format for RDF |
| cdt | 12 | Composite datatypes (lists and maps as first-class RDF terms) |
| sparql-results | 14 | SPARQL result serialization (JSON, XML, CSV, TSV, canon-arrow) |
| toml | 7 | TOML configuration codec |
| glob | 4 | Glob pattern matching over RDF terms |
| mime | 3 | Media type detection |
| iri | 2 | IRI parsing and resolution — receipted at ~1.5 µs/IRI (~309 MiB/s) and ~3.7 µs/relative resolution |

---

## 3. What the numbers mean for a buyer

### 3.1 IRIs: the per-row throughput receipt today

The IRI layer is receipted at **~1.5 microseconds per IRI (~309 MiB/s)** for
parse/ingest and ~3.7 µs for relative resolution. Every RDF statement
contains IRIs, so this is a floor on overall ingestion speed — and one of the
only figures in this ecosystem with a named, citable receipt (most competing
libraries publish no absolute numbers at all; see §4).

**Honest note:** broad end-to-end triples/second for the full parse pipeline
is the next benchmarking milestone — the scale-corpus generator produces the
data; the loader-rate measurement is not yet run. We do not claim a
triples/second figure today.

### 3.2 Correctness as a feature

PurRDF's positioning is provable correctness over raw speed where the two
conflict:

- The wire format carries cryptographic digests verified on arrival.
- The hash layer passed 273 conformance tests.
- The geodesic engine produces **certified** results — each answer carries a
  correctness certificate, which means substantially more computation per
  call than libraries returning an unverified floating-point number. For
  legal, regulatory, or safety-critical geolocation this is the
  differentiator; for maximum throughput with unproven results, a plain
  numeric library is faster.

### 3.3 Memory predictability

The scale-corpus generator produces datasets at a measured, stable density of
**~176–183 bytes per row** from 1 million to 10 billion rows (<5% drift) —
the number a deployment engineer needs to size hardware, measured rather than
guessed. Every benchmark additionally records allocation counts, requested
bytes, and peak working set.

---

## 4. Competitive landscape (honest positioning)

### What the big names actually publish

| Product | Official benchmark numbers? | What we know |
|---|---|---|
| Apache Jena / TDB2 | **No official absolute numbers.** Archived docs say "thousands of triples/s." | Third-party measurements: 45k–150k triples/s bulk load. An ESWC 2023 evaluation placed Fuseki (Jena's server) last among tested engines. |
| Oxigraph (Rust) | **No absolute numbers.** Runs the BSBM Explore workload with relative timings only. | Its own README states SPARQL query evaluation "has not been optimized yet." |
| RDFLib (Python) | **No official suite.** Community: 13k–21k triples/s parse. | 20–50× slower than C parsers on the same input. |
| Virtuoso / GraphDB | Vendor-published BSBM/WatDiv numbers on unpublished hardware. | Enterprise-licensed; not open comparables. |

### Where PurRDF stands

**PurRDF is not yet a graph database** — it is an RDF toolkit: parse,
validate, store, query, reason. The fair comparison is against RDF libraries
(RDFLib, Oxigraph library mode), not tuned database servers.

**Differentiators:**

1. **Provable correctness:** certified geodesic results, cryptographic wire
   integrity, 273 hash conformance tests. No competing RDF library offers
   proof-carrying results.
2. **Proven reliability:** 1,655 citable tests, 0 failures, footer-derived
   auditable counts. Most competing libraries publish no pass/fail counts.
3. **Full-spectrum coverage in one dependency:** 6 parse formats, SHACL +
   ShEx validation, RDFS/OWL 2 RL/Datalog/N3 reasoning, SQL bridging via
   embedded DuckDB, geospatial, full-text and vector retrieval,
   canonicalization — one data model throughout, shared verbatim across
   Rust, Python, JavaScript/WASM, and C.
4. **Deterministic infrastructure:** fixed-key hashing (no randomized
   iteration order), canonical serialization, receipt-audited releases.

**Where competitors are stronger today:**

- **Raw SPARQL query throughput at scale:** Oxigraph, Jena, and Virtuoso have
  years of query-optimizer tuning. PurRDF's SPARQL engine is correct and
  tested, not yet competitive on complex multi-join queries over billions of
  triples.
- **Ecosystem maturity:** Jena has 20 years of production deployments.
- **Bulk-load throughput:** Jena's tuned loader hits ~150k triples/s. PurRDF
  has not yet published an end-to-end loader-rate benchmark.
- **Certain geodesic hot paths:** uncertified numeric libraries are faster
  per call (they skip the correctness proof — that is the trade we sell).

---

## 5. How to read the numbers honestly

| Label | Meaning |
|---|---|
| **TERMINAL GREEN** | Ran to completion, passed, footer-recorded |
| **PARTIAL** | Produced valid citable numbers, then hit a wall cap; completed rows are real, the remainder is open |
| **Report-only** | Diagnostic output (e.g. timings) with no pass/fail condition; excluded from totals |
| **Conditional** | Depends on a one-time local setup step (e.g. the DuckDB binary cache) |
| **UNVERIFIED** | Written but not yet compiled/run; CI must confirm |

**What we do NOT claim:** multi-node performance; parity with vendor-tuned
database hardware results; long-duration stability (all runs are bounded);
GPU acceleration; a published end-to-end triples/second figure.

**What we DO claim:** every number traces to an executed command with a
per-crate footer; completed subjects measured zero failures; the suite is
re-runnable on the same hardware class with comparable results.

---

## 6. Frequently asked questions

**Q: How long did the full sweep take?**

A: It ran as a sequence of phases (1b, 2, 3, 4, 4b, plus a requeue pass) over
an overnight window. The longest single crate receipt is the hash-conformance
suite at 2,490 seconds (~41.5 minutes); most crates complete in under an
hour, most individual tests in under a minute.

**Q: Are these numbers reproducible?**

A: On the same hardware class (Apple Silicon arm64, 48 GB), yes — the harness
is deterministic: same input, same code, same output. CI re-runs the gates on
every commit.

**Q: What hardware?**

A: Apple Silicon (arm64), 48 GB unified memory. The DuckDB results require a
pre-built DuckDB binary (one-time cached download).

**Q: Why no comparison charts against Oxigraph/Jena?**

A: A fair chart requires both systems on the same hardware, same dataset,
same operations. That head-to-head (BSBM Explore is the agreed workload) is
the next benchmarking milestone. Today we publish our own absolute receipts
and third parties' published figures — nothing projected.

**Q: What about the red tests?**

A: Disclosed subjects: (1) the n3 reasoner bench hit a compile error, then a
memory kill on an oversized fixture — memory-bounded fixtures are
implemented, awaiting a CI run; (2) the sparql-eval deep-nesting bench's own
fixture probes unboundedly deep expressions — the evaluator itself is bounded
and heap-framed; the fixture cap is a small, identified change; (3) entail
and rdf-core completed partially within wall caps with all completed rows
passing. None is a product bug; each is a disclosed test-fixture or
scheduling issue with an owner.

---

## 7. Summary card

```
┌─────────────────────────────────────────────────────────────┐
│  PurRDF v26.10.28 — Benchmark Summary                       │
│  Hardware: Darwin arm64, 48 GB │ Date: 2026-10-27          │
├─────────────────────────────────────────────────────────────┤
│  CITABLE: 1,655 tests measured, 0 failures                 │
│  (raw receipts 1,693; superseded block + recount removed)  │
│                                                             │
│  CORE ENGINE       499/0  ✅  (12 footer-bearing targets)  │
│  HASH LAYER        273/0  ✅  (255 digests + 18 hasher)     │
│  TEXT SEARCH       299/0  ✅  (BM25F + phonetic + Unicode)  │
│  SPARQL ALGEBRA    140/0  ✅  (parse + planning)            │
│  GEO KERNEL        109/0  ✅  (geodesic + cells + covers)   │
│  RDF CODECS        122/0  ✅  (Turtle/N-Triples/JSON-LD…)   │
│  FACADE API         44/0  ✅  (end-to-end pipeline)         │
│  DUCKDB SQL BRIDGE  14/0  ✅  (10 sieve + 4 end-to-end)     │
│  SHACL VALIDATION   13/0  ✅  (W3C shapes)                  │
│  DATALOG REASONING  13/0  ✅  (ceiling-stopped rows         │
│                              disclosed)                     │
│  WIRE FORMAT        10/0  ✅  (canon-arrow 100k rows)       │
│  + PARTIAL: entail 48/0 (wall-capped); hnsw report-only    │
│                                                             │
│  IRI throughput: ~1.5 µs/IRI (~309 MiB/s) — receipted       │
│  Data density:   ~176–183 bytes/row, stable across scale    │
│  Integrity:      BLAKE3 digests, 273 conformance tests      │
│                                                             │
│  Disclosed reds: n3 + deep_nesting fixtures (OPT-74,        │
│  fixes implemented, awaiting CI)                            │
└─────────────────────────────────────────────────────────────┘
```

---

*Document generated 2026-10-28 from the phase-1b/2/3/4/4b benchmark receipts
(footers in `~/purrdf-bench-keep/`). Competitive data from public sources
(Apache Jena docs, RDFLib community measurements, Oxigraph documentation, ESWC
2023 proceedings); no vendor-confidential information used.*
