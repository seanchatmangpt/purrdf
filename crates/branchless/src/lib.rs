//! Branchless, allocation-free integer primitives.
//!
//! Ported from BranchlessCInRust (`bcinr`) v26.10.11 — a `no_std`,
//! `forbid(unsafe_code)` branchless-calculus library — with scaffold ABIs
//! stripped (`#[no_mangle]`, `(val, aux)` pairs) and vacuous-mutant test
//! scaffolding excluded. Every public item is constant-shape: no data-dependent
//! branches, no heap, no ambient state.
//!
//! Provenance: each module carries a `// from bcinr <path>:<lines>` header.
//! bcinr repo state: branch `bench/rdtsc-tick-tables`, commit `1e92b213`.
//!
//! One home per job: hashing lives in [`purrdf-hash`] (BLAKE3/FNV/SplitMix are
//! deliberately **not** duplicated here); lexical SIMD scanning stays in
//! `purrdf-lex::scan`; bulk bit-packed vectors stay in `purrdf-core`'s pack
//! layer. This crate owns only the shared kernel primitives.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bitmap;
pub mod bitset;
pub mod char_class;
pub mod circuit_breaker;
pub mod cost_math;
pub mod eytzinger;
pub mod filter;
pub mod hash;
pub mod packing;
pub mod saturation;
pub mod scaling;
pub mod search;
pub mod select;
pub mod simd_fallback;
pub mod sort16;
pub mod sort_small;
pub mod structural;
pub mod swar;
