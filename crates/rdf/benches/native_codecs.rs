// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! Native RDF codec hot-path benchmark.
//!
//! Report-only, `cargo bench -p purrdf --bench native_codecs`. The fixture is
//! deterministic N-Quads with default and named graph rows so both parser and
//! serializer exercise dataset-capable paths.
//!
//! Every parse row reports `Throughput::Bytes` over its input document and every
//! serialize row over its output document, so the harness prints a GB/s-class
//! rate beside the time; the byte count is measured by one untimed serialization
//! of the same document at group setup (the serializers are deterministic, so
//! the measurement is the iteration's own byte count). Per-format parse rows
//! cover every reader this crate ships: N-Triples, TriG, RDF/XML, TriX,
//! HexTuples and YAML-LD beside the N-Quads and Turtle rows, and a
//! numeric-heavy fixture (30% typed-literal rows over integer / decimal /
//! double) drives the typed-literal parse and `^^`-datatype serialize paths.

use purrdf_rdf::native_codecs::jsonld::{
    CompiledJsonLdContext, JsonLdSerializeOptions, derive_jsonld_context, parse_jsonld,
    serialize_dataset_to_jsonld, serialize_dataset_to_jsonld_with_options,
};
use purrdf_rdf::{
    NativeRdfFormat, ParseOptions, RdfDataset, RdfDatasetBuilder, RdfLiteral, SerializeGraph,
    SerializeOptions, StatementLayer, parse_dataset, parse_dataset_from_reader, parse_dataset_with,
    serialize_dataset, serialize_dataset_to_writer_with,
};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};

#[path = "support/jsonld.rs"]
mod jsonld_fixture;

use jsonld_fixture::{LARGE_ROWS as JSONLD_LARGE_ROWS, SMALL_ROWS as JSONLD_SMALL_ROWS};

const ROWS: usize = 2_000;

/// Rows for the parallel-vs-sequential parse comparison: ~50k rows is ~4.6 MiB,
/// comfortably above the 1 MiB chunk-parallel threshold in
/// `native_codecs::text_parse` while keeping the in-memory fixture small.
const LARGE_ROWS: usize = 50_000;

fn nquads_fixture(rows: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(rows * 140);
    for idx in 0..rows {
        let _ = writeln!(
            out,
            "<https://example.org/s{idx}> <https://example.org/p> \"{idx}\" <https://example.org/g{}> .",
            idx % 8
        );
    }
    out
}

fn bench_parse_nquads(c: &mut Bench) {
    let text = nquads_fixture(ROWS);
    let mut group = c.benchmark_group("native_codecs_parse");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nquads_2k", |bencher| {
        bencher.iter(|| {
            let dataset = parse_dataset(black_box(text.as_bytes()), "application/n-quads", None)
                .expect("parse");
            black_box(dataset);
        });
    });
    group.finish();
}

/// Chunk-parallel vs forced-sequential N-Quads parse over the SAME large fixture.
/// `parse_dataset` auto-selects the parallel path above the 1 MiB threshold;
/// `parse_dataset_forced_sequential` (bench-only, `#[doc(hidden)]`) pins the
/// single-threaded pipeline as the baseline. Outputs are byte-identical (the
/// determinism tests in `native_codecs::text_parse` are the gate); only wall time
/// differs.
fn bench_parse_nquads_parallel_vs_sequential(c: &mut Bench) {
    let text = nquads_fixture(LARGE_ROWS);
    let mut group = c.benchmark_group("native_codecs_parse_50k");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nquads_50k_sequential", |bencher| {
        bencher.iter(|| {
            let dataset = purrdf_rdf::native_codecs::parse_dataset_forced_sequential(
                black_box(text.as_bytes()),
                "application/n-quads",
                None,
            )
            .expect("parse");
            black_box(dataset);
        });
    });
    group.bench_function("nquads_50k_parallel", |bencher| {
        bencher.iter(|| {
            let dataset = parse_dataset(black_box(text.as_bytes()), "application/n-quads", None)
                .expect("parse");
            black_box(dataset);
        });
    });
    group.finish();
}

/// Streamed vs. buffered N-Quads parse, over the same document.
///
/// REPORT-ONLY, like every bench here, and doubly so for this pair: the streamed side
/// is measured over an in-memory `Cursor`, so it is NOT reading a file and the numbers
/// say nothing about I/O. What the pair is for is confirming that consuming the source
/// one line at a time does not cost throughput out of proportion — the streaming lane's
/// claim is about RESIDENCY, which `stream_parse_alloc` measures instead, and which no
/// timing here would show.
fn bench_parse_nquads_streamed_vs_buffered(c: &mut Bench) {
    let text = nquads_fixture(LARGE_ROWS);
    let mut group = c.benchmark_group("native_codecs_parse_50k_stream");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nquads_50k_buffered", |bencher| {
        bencher.iter(|| {
            let dataset = parse_dataset(black_box(text.as_bytes()), "application/n-quads", None)
                .expect("parse");
            black_box(dataset);
        });
    });
    group.bench_function("nquads_50k_streamed", |bencher| {
        bencher.iter(|| {
            let dataset = parse_dataset_from_reader(
                std::io::Cursor::new(black_box(text.as_bytes())),
                "application/n-quads",
                None,
            )
            .expect("parse");
            black_box(dataset);
        });
    });
    group.finish();
}

/// N-Quads fixture whose literals carry quote / backslash / tab / newline / C0-control
/// escapes. Parsing decodes them to raw chars, so serialization drives the escape
/// fast-path's BOUNDARY branch (not just the clean-run copy the numeric fixture
/// exercises). Every escape is a valid N-Quads ECHAR/UCHAR so the fixture round-trips.
fn nquads_fixture_escape_heavy(rows: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(rows * 180);
    for idx in 0..rows {
        let _ = writeln!(
            out,
            "<https://example.org/s{idx}> <https://example.org/p> \"q\\\"{idx}\\\" back\\\\ tab\\t nl\\n ctl\\u0001\" <https://example.org/g{}> .",
            idx % 8
        );
    }
    out
}

/// A const-seed xorshift64 wrapper: the fixture generator's only source of
/// variation. The mixing arithmetic is single-homed in
/// `purrdf_testkit::rng::xorshift64_next` — this bench owns only the seed.
/// Deterministic across runs and platforms; no `rand` crate, no clock (the
/// fixture law: seeded generators only).
struct XorShift64Star(u64);

impl XorShift64Star {
    /// Arbitrary fixed seed — same seed, same fixture, every run.
    const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

    const fn new() -> Self {
        Self(Self::SEED)
    }

    fn next_u64(&mut self) -> u64 {
        purrdf_testkit::rng::xorshift64_next(&mut self.0)
    }
}

/// Exactly `NUMERIC_TYPING_NUM` of every `NUMERIC_TYPING_DEN` rows (3 of 10 =
/// 30%) carry a numeric typed literal; the rest carry a plain literal.
const NUMERIC_TYPING_NUM: usize = 3;
const NUMERIC_TYPING_DEN: usize = 10;

/// The XSD numeric datatypes the typed rows cycle through.
const NUMERIC_DATATYPES: [&str; 3] = [
    "http://www.w3.org/2001/XMLSchema#integer",
    "http://www.w3.org/2001/XMLSchema#decimal",
    "http://www.w3.org/2001/XMLSchema#double",
];

/// Numeric-heavy N-Quads fixture: 30% of rows carry a numeric typed literal
/// object (`"…"^^<xsd:integer|decimal|double>`, valid XSD lexical forms with
/// xorshift-varied digit counts), 70% a plain literal. Parsing drives the
/// quoted-literal + `^^` datatype-IRI read path and the typed-literal intern;
/// serialization drives the datatype-IRI write beside the escape scan's clean
/// digit copy. Deterministic: the seed is const and the row pattern is `idx`.
fn nquads_fixture_numeric_heavy(rows: usize) -> String {
    use std::fmt::Write as _;
    let mut rng = XorShift64Star::new();
    let mut out = String::with_capacity(rows * 190);
    for idx in 0..rows {
        let typed = idx % NUMERIC_TYPING_DEN < NUMERIC_TYPING_NUM;
        let object = if typed {
            match idx % NUMERIC_DATATYPES.len() {
                0 => format!("\"{}\"^^<{}>", rng.next_u64() % 1_000_000_000, NUMERIC_DATATYPES[0]),
                1 => format!(
                    "\"{}.{}\"^^<{}>",
                    rng.next_u64() % 1_000_000,
                    rng.next_u64() % 1_000_000,
                    NUMERIC_DATATYPES[1]
                ),
                _ => format!(
                    "\"{}.{}E{}\"^^<{}>",
                    1 + (rng.next_u64() % 9),
                    rng.next_u64() % 1_000_000,
                    rng.next_u64() % 300,
                    NUMERIC_DATATYPES[2]
                ),
            }
        } else {
            format!("\"reading {idx} on graph {}\"", idx % 8)
        };
        let _ = writeln!(
            out,
            "<https://example.org/s{idx}> <https://example.org/measure> {object} <https://example.org/g{}> .",
            idx % 8
        );
    }
    out
}

/// Span-tracking OFF vs ON over the SAME N-Quads fixture, REPORT-ONLY. The `off`
/// arm is `parse_dataset_with(track_source_spans=false)`, which threads the
/// zero-sized `NoSpans` collector — the same disabled-recording path
/// `parse_dataset` compiles to; the `on` arm sets `track_source_spans=true`,
/// forcing the sequential line pipeline and populating a `SpanTable`. This exists
/// so the `off` path can be OBSERVED in the harness report to be unchanged; it
/// asserts NOTHING about relative timing (the machine is not quiet and numbers are
/// indicative only) — the byte-identical dataset guarantee is proven by the
/// `parse::tests` (`tracking_off_returns_no_table`, `dataset_is_identical_with_tracking`).
fn bench_parse_nquads_span_tracking(c: &mut Bench) {
    let text = nquads_fixture(ROWS);
    let mut group = c.benchmark_group("native_codecs_parse_span_tracking");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nquads_2k_spans_off", |bencher| {
        bencher.iter(|| {
            let options = ParseOptions {
                track_source_spans: false,
            };
            let outcome = parse_dataset_with(
                black_box(text.as_bytes()),
                "application/n-quads",
                None,
                black_box(&options),
            )
            .expect("parse");
            black_box(outcome);
        });
    });
    group.bench_function("nquads_2k_spans_on", |bencher| {
        bencher.iter(|| {
            let options = ParseOptions {
                track_source_spans: true,
            };
            let outcome = parse_dataset_with(
                black_box(text.as_bytes()),
                "application/n-quads",
                None,
                black_box(&options),
            )
            .expect("parse");
            black_box(outcome);
        });
    });
    group.finish();
}

/// Parse of the numeric-heavy fixture: the typed-literal read path (`^^`
/// datatype IRI + quoted lexical form) at 30% row density, beside the plain
/// literals. Byte throughput over the input document.
fn bench_parse_nquads_numeric_heavy(c: &mut Bench) {
    let text = nquads_fixture_numeric_heavy(ROWS);
    let mut group = c.benchmark_group("native_codecs_parse_numeric");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nquads_2k_numeric_heavy", |bencher| {
        bencher.iter(|| {
            let dataset = parse_dataset(black_box(text.as_bytes()), "application/n-quads", None)
                .expect("parse");
            black_box(dataset);
        });
    });
    group.finish();
}

/// One parse of the SAME numeric-heavy dataset spelled in every format whose
/// reader this crate ships (each row's document is produced by that format's
/// own serializer at group setup, OUTSIDE the timed region, so the parse
/// measures the reader over real per-format bytes — never a fixture builder).
/// Covers the readers the docs claim but no bench exercised: RDF/XML, TriX,
/// HexTuples and YAML-LD beside N-Triples and TriG. Report-only; the per-format
/// determinism and round-trip contracts stay with the codec tests.
fn bench_parse_formats(c: &mut Bench) {
    let dataset = parse_dataset(
        nquads_fixture_numeric_heavy(ROWS).as_bytes(),
        "application/n-quads",
        None,
    )
    .expect("numeric fixture parses");
    let mut group = c.benchmark_group("native_codecs_parse_formats");
    for (name, media_type, selection) in [
        (
            "ntriples_2k_numeric",
            "application/n-triples",
            SerializeGraph::DefaultGraph,
        ),
        ("trig_2k_numeric", "application/trig", SerializeGraph::Dataset),
        (
            "rdfxml_2k_numeric",
            "application/rdf+xml",
            SerializeGraph::DefaultGraph,
        ),
        ("trix_2k_numeric", "application/trix", SerializeGraph::Dataset),
        (
            "hextuples_2k_numeric",
            "application/x-hextuples",
            SerializeGraph::Dataset,
        ),
        (
            "yamlld_2k_numeric",
            "application/ld+yaml",
            SerializeGraph::Dataset,
        ),
    ] {
        let document = serialize_dataset(&dataset, media_type, selection).expect("fixture bytes");
        group.throughput(Throughput::Bytes(document.len() as u64));
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                let parsed = parse_dataset(black_box(document.as_slice()), media_type, None)
                    .expect("parse");
                black_box(parsed);
            });
        });
    }
    group.finish();
}

fn bench_serialize_nquads(c: &mut Bench) {
    let clean = nquads_fixture(ROWS);
    let clean_ds = parse_dataset(clean.as_bytes(), "application/n-quads", None).expect("parse");
    let dirty = nquads_fixture_escape_heavy(ROWS);
    let dirty_ds = parse_dataset(dirty.as_bytes(), "application/n-quads", None).expect("parse");
    let numeric = nquads_fixture_numeric_heavy(ROWS);
    let numeric_ds =
        parse_dataset(numeric.as_bytes(), "application/n-quads", None).expect("parse");

    // One untimed serialization per document measures the byte count each row's
    // `Throughput::Bytes` reports. The serializers are deterministic, so this is
    // the byte count of every timed iteration.
    let clean_bytes = serialize_dataset(&clean_ds, "application/n-quads", SerializeGraph::Dataset)
        .expect("serialize")
        .len() as u64;
    let dirty_bytes = serialize_dataset(&dirty_ds, "application/n-quads", SerializeGraph::Dataset)
        .expect("serialize")
        .len() as u64;
    let numeric_bytes =
        serialize_dataset(&numeric_ds, "application/n-quads", SerializeGraph::Dataset)
            .expect("serialize")
            .len() as u64;

    let mut group = c.benchmark_group("native_codecs_serialize");
    // Clean literals: the escape scan finds no boundary, one wholesale copy per literal.
    group.throughput(Throughput::Bytes(clean_bytes));
    group.bench_function("nquads_2k", |bencher| {
        bencher.iter(|| {
            let bytes = serialize_dataset(
                black_box(&clean_ds),
                "application/n-quads",
                SerializeGraph::Dataset,
            )
            .expect("serialize");
            black_box(bytes);
        });
    });
    // Escape-heavy literals: every literal has multiple boundary chars, exercising the
    // per-char fallback interleaved with clean-run copies.
    group.throughput(Throughput::Bytes(dirty_bytes));
    group.bench_function("nquads_2k_escape_heavy", |bencher| {
        bencher.iter(|| {
            let bytes = serialize_dataset(
                black_box(&dirty_ds),
                "application/n-quads",
                SerializeGraph::Dataset,
            )
            .expect("serialize");
            black_box(bytes);
        });
    });
    // Numeric-heavy literals: 30% of rows carry a typed numeric literal, so the
    // datatype-IRI write path runs beside the escape scan's clean digit copy.
    group.throughput(Throughput::Bytes(numeric_bytes));
    group.bench_function("nquads_2k_numeric_heavy", |bencher| {
        bencher.iter(|| {
            let bytes = serialize_dataset(
                black_box(&numeric_ds),
                "application/n-quads",
                SerializeGraph::Dataset,
            )
            .expect("serialize");
            black_box(bytes);
        });
    });
    // The STREAMED spelling of the same documents, into a discarding writer.
    //
    // Report-only, and the PAIRING is the point rather than either number: the eager
    // arm's cost includes growing and handing back a document-sized `Vec`, the
    // streamed arm's does not, and a reader comparing them is comparing the same
    // emitter against two destinations. There is deliberately no assertion — this
    // machine is not quiet enough for a wall-clock claim, and the memory claims this
    // work rests on are made by allocator tests rather than by timings. The byte
    // throughput is the document's measured size, identical to the eager row's.
    for (name, dataset, bytes) in [
        ("nquads_2k_streamed", &clean_ds, clean_bytes),
        ("nquads_2k_escape_heavy_streamed", &dirty_ds, dirty_bytes),
        ("nquads_2k_numeric_heavy_streamed", &numeric_ds, numeric_bytes),
    ] {
        group.throughput(Throughput::Bytes(bytes));
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                let report = serialize_dataset_to_writer_with(
                    black_box(dataset),
                    NativeRdfFormat::NQuads,
                    None,
                    &SerializeOptions {
                        selection: SerializeGraph::Dataset,
                        statement_layer: StatementLayer::PerFormatCapability,
                        jsonld_options: None,
                    },
                    &mut std::io::sink(),
                )
                .expect("serialize");
                black_box(report.bytes_written);
            });
        });
    }
    group.finish();
}

/// Pre-change expanded JSON-LD parse/serialize timing over one deterministic RDF 1.2
/// fixture at two scales. Allocation and peak-memory metrics live in the separate
/// `jsonld_alloc` process so allocator atomics cannot perturb the harness's timings.
fn bench_jsonld_expanded(c: &mut Bench) {
    let mut group = c.benchmark_group("jsonld_expanded_baseline");
    for rows in [JSONLD_SMALL_ROWS, JSONLD_LARGE_ROWS] {
        let dataset = jsonld_fixture::build_dataset(rows);
        let json = serialize_dataset_to_jsonld(&dataset).expect("prepare expanded JSON-LD");
        // Both rows process the SAME document: the serialize row writes its bytes,
        // the parse row reads them, so one byte count serves both rates.
        group.throughput(Throughput::Bytes(
            u64::try_from(json.len()).expect("document length fits in u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("serialize", rows),
            &dataset,
            |bencher, ds| {
                bencher.iter(|| {
                    let output = serialize_dataset_to_jsonld(black_box(ds))
                        .expect("expanded JSON-LD serialization");
                    black_box(output);
                });
            },
        );
        group.bench_with_input(BenchmarkId::new("parse", rows), &json, |bencher, text| {
            bencher.iter(|| {
                // No base: the generated corpus spells every IRI absolutely, so
                // resolution never runs and this measures expansion, not base
                // arithmetic.
                let dataset =
                    parse_jsonld(black_box(text.as_bytes()), None).expect("expanded JSON-LD parse");
                black_box(dataset);
            });
        });
        eprintln!(
            "[jsonld_baseline] rows={rows} quads={} output_bytes={}",
            dataset.quad_count(),
            json.len()
        );
    }
    group.finish();
}

/// Configured context compilation and caller/derived serialization are measured
/// separately so context reuse is visible rather than hidden in codec throughput.
fn bench_jsonld_configured(c: &mut Bench) {
    use std::sync::Arc;

    const CONTEXT_PREFIXES: [(&str, &str); 3] = [
        ("ex", "https://example.org/"),
        ("p", "https://example.org/p/"),
        ("o", "https://example.org/o/"),
    ];
    {
        let mut compile = c.benchmark_group("jsonld_context_compile");
        compile.throughput(Throughput::Elements(CONTEXT_PREFIXES.len() as u64));
        compile.bench_function("prefixes", |bencher| {
            bencher.iter(|| {
                let context =
                    CompiledJsonLdContext::from_prefixes(black_box(CONTEXT_PREFIXES.clone()))
                        .expect("compile context");
                black_box(context);
            });
        });
        compile.finish();
    }

    let context = Arc::new(
        CompiledJsonLdContext::from_prefixes(CONTEXT_PREFIXES).expect("compile context"),
    );
    let caller = JsonLdSerializeOptions::compiled(context);
    let derived = JsonLdSerializeOptions::derived();
    let mut group = c.benchmark_group("jsonld_configured");
    for rows in [JSONLD_SMALL_ROWS, JSONLD_LARGE_ROWS] {
        let dataset = jsonld_fixture::build_dataset(rows);
        for (mode, options) in [("caller", &caller), ("derived", &derived)] {
            let prepared = serialize_dataset_to_jsonld_with_options(&dataset, options)
                .expect("prepare configured JSON-LD");
            // Both rows process the SAME prepared document: one byte count for both
            // rates, exactly as in `jsonld_expanded_baseline`.
            group.throughput(Throughput::Bytes(
                u64::try_from(prepared.len()).expect("document length fits in u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(format!("serialize_{mode}"), rows),
                &dataset,
                |bencher, value| {
                    bencher.iter(|| {
                        black_box(
                            serialize_dataset_to_jsonld_with_options(black_box(value), options)
                                .expect("configured JSON-LD serialization"),
                        );
                    });
                },
            );
            group.bench_with_input(
                BenchmarkId::new(format!("parse_{mode}"), rows),
                &prepared,
                |bencher, text| {
                    bencher.iter(|| {
                        // No base, as above: the corpus is absolute-only by
                        // construction.
                        black_box(
                            parse_jsonld(black_box(text.as_bytes()), None)
                                .expect("configured JSON-LD parse"),
                        );
                    });
                },
            );
            eprintln!(
                "[jsonld_configured] mode={mode} rows={rows} quads={} output_bytes={}",
                dataset.quad_count(),
                prepared.len()
            );
        }
    }
    group.finish();
}

/// Context derivation has an explicit work ceiling; this adversarial shape keeps
/// many profitable namespaces and distinct IRIs below it while exposing scaling.
fn bench_jsonld_derived_many_namespaces(c: &mut Bench) {
    const NAMESPACES: usize = 64;
    const IRIS_PER_NAMESPACE: usize = 32;

    let dataset = jsonld_fixture::build_many_namespace_dataset(NAMESPACES, IRIS_PER_NAMESPACE);
    let mut group = c.benchmark_group("jsonld_derived_context");
    group.sample_size(10);
    group.throughput(Throughput::Elements(
        u64::try_from(dataset.quad_count()).expect("quad count fits in u64"),
    ));
    group.bench_function("64_namespaces_32_iris", |bencher| {
        bencher.iter(|| {
            black_box(derive_jsonld_context(black_box(&dataset)).expect("derive bounded context"));
        });
    });
    group.finish();
}

/// Named-graph usage accounting and multivalue ordering are independent carrier
/// hot paths, so keep adversarial shapes for both in the report-only benchmark.
fn bench_jsonld_carrier_stress(c: &mut Bench) {
    let context = std::sync::Arc::new(
        CompiledJsonLdContext::from_prefixes([("bench", "https://bench.example/")])
            .expect("compile benchmark context"),
    );
    let options = JsonLdSerializeOptions::compiled(context);
    let mut group = c.benchmark_group("jsonld_carrier_stress");
    group.sample_size(10);
    for (name, dataset) in [
        (
            "named_graphs_512",
            jsonld_fixture::build_many_named_graph_dataset(512),
        ),
        (
            "multivalue_4096",
            jsonld_fixture::build_multivalue_dataset(4_096),
        ),
    ] {
        // One untimed serialization measures the row's byte count.
        let bytes = serialize_dataset_to_jsonld_with_options(&dataset, &options)
            .expect("measure fixture bytes")
            .len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(name, &dataset, |bencher, dataset| {
            bencher.iter(|| {
                black_box(
                    serialize_dataset_to_jsonld_with_options(black_box(dataset), &options)
                        .expect("serialize carrier stress fixture"),
                );
            });
        });
    }
    group.finish();
}

/// A ~2k-quad RDF 1.2 dataset for the text-family / RDF-XML / JSON-LD serializers:
/// typed (`xsd:integer`, `xsd:dateTime`) and language-tagged literals on every row, a
/// named graph on every fourth, and — when `star` is set — a quoted-triple object plus
/// a reifier row and an annotation on every eighth. Every predicate repeats across all
/// rows, so the interner's id memo answers most term occurrences.
fn star_fixture(rows: usize, star: bool) -> std::sync::Arc<RdfDataset> {
    let mut builder = RdfDatasetBuilder::new();
    let knows = builder.intern_iri("https://example.org/knows").unwrap();
    let count = builder.intern_iri("https://example.org/count").unwrap();
    let seen = builder.intern_iri("https://example.org/seen").unwrap();
    let label = builder.intern_iri("https://example.org/label").unwrap();
    let asserts = builder.intern_iri("https://example.org/asserts").unwrap();
    let confidence = builder
        .intern_iri("https://example.org/confidence")
        .unwrap();
    let graph = builder.intern_iri("https://example.org/graph").unwrap();
    for index in 0..rows {
        let subject = builder
            .intern_iri(&format!("https://example.org/s{index}"))
            .unwrap();
        let object = builder
            .intern_iri(&format!("https://example.org/o{}", index % 64))
            .unwrap();
        let integer = builder
            .intern_literal(RdfLiteral::typed(
                index.to_string(),
                "http://www.w3.org/2001/XMLSchema#integer",
            ))
            .unwrap();
        let stamp = builder
            .intern_literal(RdfLiteral::typed(
                format!("2026-01-01T00:00:{:02}Z", index % 60),
                "http://www.w3.org/2001/XMLSchema#dateTime",
            ))
            .unwrap();
        let tagged = builder
            .intern_literal(RdfLiteral::language_tagged(
                format!("label {index}"),
                if index % 2 == 0 { "en" } else { "de" },
            ))
            .unwrap();
        let graph_name = (index % 4 == 0).then_some(graph);
        builder
            .push_quad(subject, knows, object, graph_name)
            .unwrap();
        builder
            .push_quad(subject, count, integer, graph_name)
            .unwrap();
        builder.push_quad(subject, seen, stamp, graph_name).unwrap();
        builder
            .push_quad(subject, label, tagged, graph_name)
            .unwrap();
        if star && index % 8 == 0 {
            let proposition = builder.intern_triple(subject, knows, object).unwrap();
            let reifier = builder
                .intern_iri(&format!("https://example.org/r{index}"))
                .unwrap();
            let score = builder
                .intern_literal(RdfLiteral::typed(
                    format!("0.{}", index % 10),
                    "http://www.w3.org/2001/XMLSchema#decimal",
                ))
                .unwrap();
            builder
                .push_reifier_in_graph(reifier, proposition, graph_name)
                .unwrap();
            builder
                .push_annotation_in_graph(reifier, confidence, score, graph_name)
                .unwrap();
            builder
                .push_quad(subject, asserts, proposition, graph_name)
                .unwrap();
        }
    }
    builder.freeze().expect("star serializer fixture")
}

/// Turtle / TriG / N-Triples / RDF-XML / TriX / HexTuples / JSON-LD / YAML-LD egress
/// over one ~2k-quad dataset with quoted-triple and reifier rows and datatyped
/// literals (HexTuples and TriX have no triple-term surface, so they take the same
/// fixture without the star rows). Each row's `Throughput::Bytes` is its own
/// document's size, measured by one untimed serialization at group setup.
/// Report-only.
fn bench_serialize_formats(c: &mut Bench) {
    let star = star_fixture(ROWS / 4, true);
    let plain = star_fixture(ROWS / 4, false);
    let mut group = c.benchmark_group("native_codecs_serialize_formats");
    for (name, media_type, selection, dataset) in [
        (
            "turtle_2k_star",
            "text/turtle",
            SerializeGraph::DefaultGraph,
            &star,
        ),
        (
            "trig_2k_star",
            "application/trig",
            SerializeGraph::Dataset,
            &star,
        ),
        (
            "ntriples_2k_star",
            "application/n-triples",
            SerializeGraph::DefaultGraph,
            &star,
        ),
        (
            "rdfxml_2k_star",
            "application/rdf+xml",
            SerializeGraph::DefaultGraph,
            &star,
        ),
        (
            "trix_2k",
            "application/trix",
            SerializeGraph::Dataset,
            &plain,
        ),
        (
            "hextuples_2k",
            "application/x-hextuples",
            SerializeGraph::Dataset,
            &plain,
        ),
        (
            "jsonld_2k_star",
            "application/ld+json",
            SerializeGraph::Dataset,
            &star,
        ),
        (
            "yamlld_2k_star",
            "application/ld+yaml",
            SerializeGraph::Dataset,
            &star,
        ),
    ] {
        let bytes = serialize_dataset(dataset, media_type, selection).expect("fixture bytes");
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                let bytes = serialize_dataset(black_box(dataset), media_type, selection)
                    .expect("serialize");
                black_box(bytes);
            });
        });
    }
    group.finish();
}

/// Turtle document whose every statement's object is an RDF collection: exercises the
/// parser's collection loop (`rdf:first` / `rdf:rest` / `rdf:nil` cell emission).
fn turtle_collections_fixture(rows: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(rows * 120);
    out.push_str("@prefix ex: <https://example.org/> .\n");
    for idx in 0..rows {
        let _ = writeln!(
            out,
            "ex:s{idx} ex:items ( \"a{idx}\" ex:o{} {} ( ex:nested \"x\" ) ) .",
            idx % 16,
            idx
        );
    }
    out
}

fn bench_parse_turtle_collections(c: &mut Bench) {
    let text = turtle_collections_fixture(ROWS);
    let mut group = c.benchmark_group("native_codecs_parse_turtle");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("turtle_2k_collections", |bencher| {
        bencher.iter(|| {
            let dataset =
                parse_dataset(black_box(text.as_bytes()), "text/turtle", None).expect("parse");
            black_box(dataset);
        });
    });
    group.finish();
}

/// A dataset where a handful of predicates and objects recur across thousands of quads,
/// so nearly every term occurrence the Turtle serializer interns is a repeat: the
/// interner's id-keyed memo answers it without materializing the term's value.
fn repeated_predicates_fixture(rows: usize) -> std::sync::Arc<RdfDataset> {
    let mut builder = RdfDatasetBuilder::new();
    let predicates: Vec<_> = (0..5)
        .map(|i| {
            builder
                .intern_iri(&format!("https://example.org/p{i}"))
                .unwrap()
        })
        .collect();
    let objects: Vec<_> = (0..8)
        .map(|i| {
            builder
                .intern_iri(&format!("https://example.org/o{i}"))
                .unwrap()
        })
        .collect();
    for index in 0..rows {
        let subject = builder
            .intern_iri(&format!("https://example.org/s{}", index / 5))
            .unwrap();
        builder
            .push_quad(
                subject,
                predicates[index % predicates.len()],
                objects[index % objects.len()],
                None,
            )
            .unwrap();
    }
    builder.freeze().expect("repeated-predicate fixture")
}

fn bench_serialize_turtle_repeated_predicates(c: &mut Bench) {
    let dataset = repeated_predicates_fixture(ROWS * 4);
    // One untimed serialization measures the byte count the rate reports.
    let bytes = serialize_dataset(&dataset, "text/turtle", SerializeGraph::DefaultGraph)
        .expect("serialize")
        .len() as u64;
    let mut group = c.benchmark_group("native_codecs_serialize_turtle");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("turtle_8k_repeated_predicates", |bencher| {
        bencher.iter(|| {
            let bytes = serialize_dataset(
                black_box(&dataset),
                "text/turtle",
                SerializeGraph::DefaultGraph,
            )
            .expect("serialize");
            black_box(bytes);
        });
    });
    group.finish();
}

bench_group!(
    benches,
    bench_parse_nquads,
    bench_parse_nquads_parallel_vs_sequential,
    bench_parse_nquads_streamed_vs_buffered,
    bench_parse_nquads_span_tracking,
    bench_parse_nquads_numeric_heavy,
    bench_parse_formats,
    bench_serialize_nquads,
    bench_serialize_formats,
    bench_parse_turtle_collections,
    bench_serialize_turtle_repeated_predicates,
    bench_jsonld_expanded,
    bench_jsonld_configured,
    bench_jsonld_derived_many_namespaces,
    bench_jsonld_carrier_stress
);
bench_main!(benches);
