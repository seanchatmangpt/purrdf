<!--
SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
SPDX-License-Identifier: CC-BY-4.0
-->

<p align="center">
  <a href="https://github.com/Blackcat-Informatics/purrdf">
    <img src="https://raw.githubusercontent.com/Blackcat-Informatics/purrdf/main/docs/purrdf-logo.svg" alt="PurRDF logo" width="120" height="120">
  </a>
</p>

# `purrdf-n3` — Native Notation3

[![crates.io](https://img.shields.io/crates/v/purrdf-n3.svg)](https://crates.io/crates/purrdf-n3)
[![docs.rs](https://docs.rs/purrdf-n3/badge.svg)](https://docs.rs/purrdf-n3)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0%20OR%20MulanPSL--2.0-blue.svg)](https://github.com/Blackcat-Informatics/purrdf/blob/main/LICENSE-MIT)
[![Repository](https://img.shields.io/badge/repo-Blackcat--Informatics%2Fpurrdf-181717.svg)](https://github.com/Blackcat-Informatics/purrdf)

Native, wasm-clean Notation3 (N3): a parser for N3's superset of Turtle — graph
formulas `{ … }`, `@forAll`/`@forSome` quantification, `=`/`<=`/`=>` paths and
`log:` vocabulary — plus semi-naive reasoning over formula-scoped rule sets and
backward (proof-carrying) chains, carrying no ambient I/O, no wall clock and no
RNG.

A formula is *data* — a scoped table of terms, triples and rules over the same
term identities the RDF 1.2 codecs mint — rather than a hand-written tree walk.
Its consumers are [`purrdf-entail`](https://crates.io/crates/purrdf-entail),
whose calculi consume the scoped rule sets this crate parses, and
[`purrdf-shapes`](https://crates.io/crates/purrdf-shapes), whose SPARQL 1.2 RL
rule text lowers onto the same schedule.

## Formula-scoped model

`@forAll`/`@forSome` bind within their enclosing formula scope: a quantified
IRI resolves to its quantifier term while that scope is open, and the binding
ends when the scope closes — the same IRI used after the closing `}` parses as
a plain IRI (a different term identity), not an error. Quantifier refusals
cover ordering and duplication only: a quantifier must precede its graph's
first triple, `@forAll` must precede `@forSome` in the same graph, and an IRI
cannot be quantified twice in one graph. Triple
terms keep the RDF 1.2 term model exactly; a formula term is a first-class term
identity of its own. `.log:` predicates are parsed as ordinary IRIs — the
*meaning* of `log:implies` and friends is supplied by the reasoner, not the
parser: a formula with unparsed `log:` payloads still round-trips as data.

## Design commitments

* **One term identity across scopes.** Every term — literal, IRI, blank node,
  formula — is minted through the shared interner, so a term quoted inside
  `{ … }` and the same term asserted at the top level are one identity, and
  scope is a property of the rule set, never of the term.
* **Reasoning is semi-naive over scoped rules.** Rule bodies that span formula
  boundaries lower onto the same ordered schedule the Datalog evaluator runs;
  no second evaluator is minted for N3.
* **Proofs are backward and content-addressed.** A backward chain from a goal
  carries the same derivation-identity recipe the reasoning lane keys on, so a
  proof can be replayed and checked independently of the forward closure.
* **Deterministic by construction.** Fixed-key hashing (`FixedHasher`, not
  `RandomState`), insertion-ordered per-key rows, and no map iteration order on
  an output path. Identical input yields byte-identical output on every target.
* **Limits refuse; they never truncate.** Any budget is the caller's and can
  only produce a typed refusal naming the limit — never a partial parse or a
  truncated model.
* **wasm-clean.** No filesystem, no clock, no RNG, no threads-only constructs.
* **No optionality.** No Cargo features; no conditional compilation selecting
  between semantics.

## Known limitations

* **Quantifier scope escape is not refused.** The parser has no check that an
  IRI bound by `@forAll`/`@forSome` stays within its scope: when the scope
  closes the binding simply ends, and a later use of the same IRI parses as a
  plain IRI with a different term identity. Callers needing escape detection
  must check it themselves (as of 2026-10-25).

## Part of PurRDF

This crate is one member of the [PurRDF](https://github.com/Blackcat-Informatics/purrdf)
workspace — an RDF 1.2 toolkit with native codecs, SPARQL, SHACL, ShEx,
entailment, and the GTS graph transport, carried into Python, WebAssembly, and
C. It is the parser and prover beneath
[`purrdf-entail`](https://crates.io/crates/purrdf-entail)'s N3 surface, and is
published separately so a caller can depend on N3 alone. Note that it is not
re-exported by the umbrella [`purrdf`](https://crates.io/crates/purrdf) crate.

There are deliberately no Cargo feature flags anywhere in the workspace. MSRV
follows the workspace `rust-version` (currently 1.98, stable toolchain only).

## License

Licensed under any one of the following, at your option:

- [MIT license](https://github.com/Blackcat-Informatics/purrdf/blob/main/LICENSE-MIT)
- [Apache License, Version 2.0](https://github.com/Blackcat-Informatics/purrdf/blob/main/LICENSE-APACHE)
- [Mulan Permissive Software License, Version 2 (MulanPSL-2.0)](https://github.com/Blackcat-Informatics/purrdf/blob/main/LICENSE-MULAN)
