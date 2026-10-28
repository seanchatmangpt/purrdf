# OPT-72: W3C vocab-literal completion sweep (wave-5 L10 receipt)

**Status:** OPEN (filed 2026-10-27, wave-5)

## Direct fixes (deps present — mechanical, do on next pass)

- crates/gts/src/examples/agent_memory.rs:30-45 →
  `vocab::rdf::{VALUE,TYPE}` + `purrdf_xsd::datatype::{XSD_DECIMAL,
  XSD_DATE_TIME}` (gts holds both deps).
- crates/sparql-algebra/src/parser.rs:7797-8406 (6 hits, not the
  :12029/:12057 fixture strings) + serialize.rs:3454 → `XSD_*`.
- crates/sparql-eval/src/row_checkpoint_gate.rs:75,288,436 →
  `XSD_INTEGER`/`XSD_STRING`.
- crates/shapes/src/free_expression.rs:224 → `vocab::rdf::VALUE`.
- crates/sql/src/compiler.rs rdf-only sites (:98-100, :138/:365/:429/:895/
  :2460/:2540/:2583) → `vocab::rdf::{FIRST,REST,NIL,TYPE}` (sql has iri).

## Dep-decision items (need purrdf-xsd/purrdf-iri added → lockfile → CI)

- duckdb eval.rs:270-343 (10 XSD literals, crate has neither dep — worst).
- sql compiler.rs XSD sites (:518-:2925, 8 hits).
- n3 parser.rs/ast.rs/printing.rs/proof_writer.rs/proof_check_n3.rs
  (~31 XSD sites total; rdf ones direct-fixable, n3 has iri).
- lex term_syntax.rs:44 (1 hit, both deps missing).
- sparql-results json_read.rs:1947 (1 hit).
- slice prefix_emit.rs:49-52 (xsd NS lines).

## Data / out-of-scope (leave)

Fixture/golden strings (sparql-algebra :12029+/:3071-3694, sparql-results
json.rs:588+, shapes typescript.rs:2185, rdfxml.rs:1318 xmlns) and
validate/regime.rs + entail.rs Turtle-document strings (low priority).

## Verification invariant

`grep -rn "2001/XMLSchema#" crates/*/src` outside cfg(test)/fixtures
returns only the constants' home (xsd/src/datatype.rs); same for the
rdf-syntax-ns and owl namespaces outside iri/src/vocab.rs.
