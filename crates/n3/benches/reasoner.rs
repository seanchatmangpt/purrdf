// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! N3 reasoner benchmarks.
//!
//! `purrdf-n3` had no bench of its own: the nearest was the datalog evaluator's
//! `seminaive` bench, which measures the join, not the parser that feeds it or
//! the proof machinery that audits its output. These five groups give the N3
//! stack its own numbers:
//!
//! * `parse` — throughput of `parse_n3` over a generated flat document of
//!   simple triples at two sizes (1k and 10k triples), so a parser change has
//!   a per-triple number that is not mixed with reasoning.
//! * `parse_mix` — the SWAR digit-run story: `parse_n3` over two fixtures of
//!   the same triple count whose token mixtures are the two ends of
//!   `Lexer::take_digit_run`'s domain. The numeric-heavy fixture spends every
//!   digit byte on number tokens with 8-to-19-digit runs (whole-word SWAR
//!   consumption); the prose-heavy fixture has no digit byte anywhere (the
//!   scan is never entered). Both rows declare `Throughput::Bytes`, so the
//!   report reads GB/s for each and the ratio between them is the digit-run
//!   share of parse cost. `--save-baseline` before a lexer revision and
//!   `--baseline` after it is the literal before/after SWAR comparison.
//! * `fixpoint` — forward chaining to convergence on two seeded rule sets:
//!   the `chain` program, where two rules close `p` into `anc` transitively
//!   over a chain so the run takes many rounds, and the `numeric_peer`
//!   program, where one join rule pairs items over numeric-literal values the
//!   parser minted through `take_digit_run`. Each row declares
//!   `Throughput::Elements` over the closure it converges to, so the rate is
//!   derived facts (proofs) per second, not per input link.
//! * `proof` — proof generation (`proof: true`) plus `proof_to_n3` and a full
//!   `check_proof` verification of the written proof, so the audit path is
//!   measured as one operation and cannot be optimized by weakening either
//!   half.
//! * `proof_check` — `check_proof` alone, ns/op: the written proof is built
//!   once per size and the routine only verifies it against the source text
//!   (`check_proof` re-parses the source, so the row costs what the public
//!   API costs). No throughput is declared; the row reads as time per
//!   verification.
//!
//! Fixtures are deterministic: fixed constants seed a small LCG in this file;
//! there is no `rand` crate and no clock. Report-only, per this repository's
//! rule: benches exist so a later change has a number to move, never so a
//! speedup can be asserted. Nothing here fails a build.

use purrdf_n3::parser::parse_n3;
use purrdf_n3::proof_check_n3::check_proof;
use purrdf_n3::proof_writer::proof_to_n3;
use purrdf_n3::reasoner::{CompletionStatus, ReasonerOptions, ReasonerResult, reason};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};

/// Seeded fixture source: delegates to the one-home LCG
/// (`purrdf_hash::mix::lcg64_next`, Knuth's MMIX constants — helpers-ledger
/// job `lcg64-mmix`). Deterministic by construction — the same seed produces
/// byte-identical text on every platform and every run, which is what makes a
/// fixture reviewable and its numbers comparable.
struct FixtureRng(u64);

impl FixtureRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        purrdf_hash::mix::lcg64_next(&mut self.0, 1)
    }
}

/// The letter alphabet IRI local names are spelled from, so fixture names
/// carry no digit byte: in the prose-heavy fixture no byte is a digit, and in
/// the numeric-heavy fixture every digit byte belongs to a number token — the
/// two mixtures stay honest by construction.
const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// The `i`-th digit-free local name: `0 -> "a"`, `25 -> "z"`, `26 -> "aa"`,
/// …, in bijective base 26.
fn letters(i: usize) -> String {
    let mut n = i as u64 + 1;
    let mut buf = Vec::new();
    while n > 0 {
        n -= 1;
        buf.push(LETTERS[(n % 26) as usize]);
        n /= 26;
    }
    buf.reverse();
    String::from_utf8(buf).expect("letter local names are ASCII")
}

/// Build the flat document `ex:s{i} ex:p ex:o{i}` of `n` triples, as text, the
/// way a caller actually feeds the parser.
fn flat_document(n: usize) -> String {
    let mut text = String::with_capacity(n * 32 + 64);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    for i in 0..n {
        text.push_str(&format!("ex:s{i} ex:p ex:o{i} .\n"));
    }
    text
}

/// Build the numeric-heavy document: `n` triples whose objects are all numeric
/// literals — 60% integers, 20% decimals, 20% doubles — under one LCG seed.
/// Every value is spelled so a digit run crosses at least one whole 8-byte
/// word (19-digit integers, 9+9-digit decimals, 8-digit mantissas), the input
/// shape `Lexer::take_digit_run`'s SWAR path exists for; the 2-digit exponent
/// runs fall to the scalar tail the way real exponents do. The audit's bar is
/// 30% numeric literals; this fixture sits at 100% of objects so the
/// digit-run share of parse time is measured at its upper end.
fn numeric_document(n: usize) -> String {
    const NUMERIC_SEED: u64 = 0x5EED_CAFE_F00D_0001;
    let mut rng = FixtureRng::new(NUMERIC_SEED);
    let mut text = String::with_capacity(n * 56 + 64);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    for i in 0..n {
        let subject = letters(i);
        // 3-in-5 integer, 1-in-5 decimal, 1-in-5 double, drawn from one LCG
        // stream so the mixture is fixed across runs and platforms.
        match rng.next_u64() % 5 {
            0..=2 => {
                // A 19-digit integer: `[1e18, 1e19)` — two whole words plus a
                // scalar tail.
                let v = 1_000_000_000_000_000_000 + rng.next_u64() % 9_000_000_000_000_000_000;
                text.push_str(&format!("ex:{subject} ex:measures {v} .\n"));
            }
            3 => {
                // A decimal with a 9-digit whole part and a 9-digit fraction:
                // two runs, each a full word plus a tail.
                let whole = 100_000_000 + rng.next_u64() % 900_000_000;
                let frac = rng.next_u64() % 1_000_000_000;
                text.push_str(&format!("ex:{subject} ex:measures {whole}.{frac:09} .\n"));
            }
            _ => {
                // A double: an 8-digit mantissa (exactly one full word) and a
                // signed 2-digit exponent.
                let mantissa = 10_000_000 + rng.next_u64() % 90_000_000;
                let exp = 10 + rng.next_u64() % 40;
                text.push_str(&format!("ex:{subject} ex:measures {mantissa}e+{exp} .\n"));
            }
        }
    }
    text
}

/// Build the prose-heavy control: `n` triples over letter-only names — no
/// digit byte anywhere — so `take_digit_run` is never entered and the parse
/// rate beside the numeric fixture is the digit-run-free end of the SWAR
/// story. Same line shape as the numeric fixture; only the token classes
/// differ.
fn prose_document(n: usize) -> String {
    let mut text = String::with_capacity(n * 44 + 64);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    for i in 0..n {
        text.push_str(&format!(
            "ex:{} ex:describes ex:{} .\n",
            letters(i),
            letters(n - 1 - i)
        ));
    }
    text
}

/// The `p` chain `ex:n0 -> ex:n1 -> … -> ex:n{n}` of `n` links, written in
/// both directions so the text carries `2n` facts, plus the two rules whose
/// fixpoint closes `p` into `anc` transitively: every node reaches every
/// other, so the closure is `n * (n + 1)` derived facts and the run takes
/// many rounds.
fn chain_program(n: usize) -> String {
    let mut text = String::with_capacity(n * 32 + 256);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    text.push_str("{ ?s ex:p ?o } => { ?s ex:anc ?o } .\n");
    text.push_str("{ ?s ex:p ?m . ?m ex:anc ?o } => { ?s ex:anc ?o } .\n");
    for i in 0..n {
        text.push_str(&format!("ex:n{i} ex:p ex:n{} .\n", i + 1));
        text.push_str(&format!("ex:n{} ex:p ex:n{i} .\n", i + 1));
    }
    text
}

/// The numeric peer rule set: `n` values drawn from an LCG seed, each held by
/// exactly two items, plus the one rule whose fixpoint pairs every two items
/// sharing a value — an item pairs with itself too, so the closure is four
/// derived facts per value. The join runs over the numeric literals
/// themselves, so the fixpoint is measured over data minted through the SWAR
/// digit-run scan, not over IRIs alone. The derived predicate (`ex:peer`)
/// never feeds a premise (`ex:val`), so the fixpoint converges
/// deterministically and never walks its own output.
fn peer_program(n: usize) -> String {
    const PEER_SEED: u64 = 0x5EED_CAFE_F00D_0002;
    let mut rng = FixtureRng::new(PEER_SEED);
    let mut text = String::with_capacity(2 * n * 56 + 256);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    text.push_str("{ ?x ex:val ?v . ?y ex:val ?v } => { ?x ex:peer ?y } .\n");
    for i in 0..n {
        let v = 1_000_000_000_000_000_000 + rng.next_u64() % 9_000_000_000_000_000_000;
        text.push_str(&format!("ex:{} ex:val {v} .\n", letters(2 * i)));
        text.push_str(&format!("ex:{} ex:val {v} .\n", letters(2 * i + 1)));
    }
    text
}

/// Run one program to fixpoint, panicking rather than reporting a partial
/// answer — a bench that silently measured a limit refusal would be measuring
/// nothing.
fn reason_fixture(program: &str) -> ReasonerResult {
    let doc = parse_n3(program, None).expect("the fixture program parses");
    let result = reason(&doc, &ReasonerOptions::default());
    assert_eq!(
        result.status,
        CompletionStatus::Complete,
        "the fixture fixpoint converges inside every ceiling"
    );
    result
}

/// Parse the fixture, run the fixpoint with proofs on, and return the written
/// proof — panicking on any partial result.
fn build_proof(program: &str) -> String {
    let doc = parse_n3(program, None).expect("the fixture program parses");
    let options = ReasonerOptions {
        proof: true,
        ..ReasonerOptions::default()
    };
    let result = reason(&doc, &options);
    assert_eq!(
        result.status,
        CompletionStatus::Complete,
        "the fixture fixpoint converges inside every ceiling"
    );
    proof_to_n3(&doc.prefixes, &result)
}

/// Parse the fixture, run the fixpoint with proofs on, write the proof to N3,
/// and verify it — panicking on any partial result. The proof bench measures
/// this whole operation: generation, serialization, verification.
fn proof_fixture(program: &str) {
    let doc = parse_n3(program, None).expect("the fixture program parses");
    let options = ReasonerOptions {
        proof: true,
        ..ReasonerOptions::default()
    };
    let result = reason(&doc, &options);
    assert_eq!(
        result.status,
        CompletionStatus::Complete,
        "the fixture fixpoint converges inside every ceiling"
    );
    let proof = proof_to_n3(&doc.prefixes, &result);
    assert!(
        !proof.is_empty(),
        "the fixture derives facts, so a proof exists"
    );
    let report = check_proof(program, &proof, "bench-reasoner").expect("the proof reads back");
    assert!(
        report.valid(),
        "the fixture's proof verifies: {}",
        report.verdict()
    );
}

/// `parse_n3` over the flat fixture at two sizes: the per-triple parser cost
/// un-mixed from reasoning.
fn parse(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_parse");
    for n in [1_000_usize, 10_000] {
        let text = flat_document(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &text, |bch, text| {
            bch.iter(|| black_box(parse_n3(black_box(text), None)).is_ok());
        });
    }
    group.finish();
}

/// `parse_n3` over the two mixtures at equal triple counts: the numeric-heavy
/// fixture spends its digit bytes on number tokens (the SWAR path), the
/// prose-heavy one has none (the scan is never entered). `Throughput::Bytes`
/// turns both rows into GB/s, so the report carries the digit-run story end
/// to end.
fn parse_mix(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_parse_mix");
    for n in [1_000_usize, 10_000] {
        for (name, text) in [
            ("numeric", numeric_document(n)),
            ("prose", prose_document(n)),
        ] {
            group.throughput(Throughput::Bytes(text.len() as u64));
            group.bench_with_input(BenchmarkId::new(name, n), &text, |bch, text| {
                bch.iter(|| black_box(parse_n3(black_box(text), None)).is_ok());
            });
        }
    }
    group.finish();
}

/// Forward chaining to convergence on the two seeded rule sets: the many-round
/// transitive closure and the numeric-literal join. The throughput is the
/// closure each program converges to, computed here at fixture-build time, so
/// the reported rate is derived facts (proofs) per second and the closure size
/// is printed in the report rather than inferred.
fn fixpoint(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_fixpoint");
    // stay inside the 30s ceiling; the top rung is gated off by default
    // because its fixture construction OOM-killed the 4b run (SIGKILL after
    // n3_parse rows, before this group printed).
    group.sample_size(20);
    let large = std::env::var("N3_FIXPOINT_LARGE").is_ok_and(|v| v == "1");
    for n in if large { [64_usize, 256] } else { [64_usize, 128] } {
        let chain = chain_program(n);
        let peer = peer_program(n);
        // Analytic closure sizes, computed without running the reasoner: a
        // fixture-build-time `reason_fixture(..).closure.len()` here
        // materializes the full closure twice per size purely to count it,
        // which OOM-killed the 48 GB box (SIGKILL during fixture
        // construction, OPT-74). `closure` holds explicit plus derived facts
        // (reasoner.rs:1223), so the counts are exact by the fixture
        // construction itself:
        //
        // * chain — the program writes `2n` explicit `p` facts (the path in
        //   both directions); the two rules close `anc` to every ordered pair
        //   of the `n + 1` path nodes except the reflexive ones (the path has
        //   no cycle, so `?s anc ?s` is never derivable): `n * (n + 1)`
        //   derived, `n * n + 3 * n` total.
        // * numeric_peer — `2n` explicit `val` facts, `n` values with two
        //   holders each; the one join rule derives both self- and cross-
        //   pairings, `4` per value: `4 * n` derived, `6 * n` total.
        let chain_closure = n * n + 3 * n;
        let peer_closure = 6 * n;
        for (name, program, closure) in [
            ("chain", chain, chain_closure),
            ("numeric_peer", peer, peer_closure),
        ] {
            group.throughput(Throughput::Elements(closure as u64));
            group.bench_with_input(BenchmarkId::new(name, n), &program, |bch, program| {
                bch.iter(|| black_box(reason_fixture(black_box(program))));
            });
            let _ = black_box(closure);
        }
    }
    group.finish();
}

/// Proof generation + `proof_to_n3` + `check_proof` verification as one
/// operation over the same fixpoint fixture.
fn proof(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_proof");
    for n in [64_usize, 256] {
        let program = chain_program(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &program, |bch, program| {
            bch.iter(|| proof_fixture(black_box(program)));
        });
    }
    group.finish();
}

/// `check_proof` alone, ns/op: the written proof is built once per size and
/// the routine verifies it against the source text — `check_proof` re-parses
/// the source, so the row costs what the public API costs. No throughput is
/// declared; the row reads as time per verification.
fn proof_check(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_proof_check");
    for n in [64_usize, 256] {
        let program = chain_program(n);
        let proof = build_proof(&program);
        group.bench_with_input(BenchmarkId::from_parameter(n), &proof, |bch, proof| {
            bch.iter(|| {
                let report = check_proof(black_box(program.as_str()), proof, "bench-reasoner")
                    .expect("the fixture proof reads back");
                assert!(
                    report.valid(),
                    "the fixture's proof verifies: {}",
                    report.verdict()
                );
            });
        });
    }
    group.finish();
}

bench_group!(benches, parse, parse_mix, fixpoint, proof, proof_check);
bench_main!(benches);
