// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Node-side cold-start benchmark for the purrdf wasm package: the wall time a
// fresh JavaScript realm pays from process entry through module init (ready)
// to the first SPARQL SELECT returning.
//
// Why a child process per sample: the package allows exactly one wasm
// instance per JavaScript realm (see ready() in index.mjs — once a realm has
// loaded the package, only a fresh realm can load it again), so a cold start
// cannot be repeated in-process. Each sample spawns a fresh `node` child that
// reports one JSON line with three spans:
//   ready_ms        process entry -> ready() resolved (compile + instantiate)
//   first_query_ms  ready() -> first SELECT result inspected
//   cold_ms         process entry -> first query, the headline number
// The parent prints the median of the samples plus the full document (every
// sample, not just the median). Report-only (never a gate): it asserts
// correctness (corpus size, row count) and never gates on the wall times.
//
// Usage:
//   node bench/coldstart.bench.mjs [--out <path>]
// Tunables (env):
//   BENCH_COLDSTART_INITS  fresh realms to measure  (default 20)

import { spawnSync } from "node:child_process";
import { accessSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseOutArg, writeResultDocument } from "./persist.mjs";

const BENCH_ROOT = dirname(fileURLToPath(import.meta.url));
const PKG_WASM = resolve(BENCH_ROOT, "../pkg/purrdf_wasm_bg.wasm");
const CHILD_FLAG = "--coldstart-child";

// Deterministic first-query corpus (fixture law: const shape, no Date, no
// randomness). Three quads per subject — IRI object, integer-typed literal,
// link/name — so the parse size and the SELECT row count are exact.
const SUBJECTS = 32;
const CORPUS = Array.from({ length: SUBJECTS }, (_, i) => {
  const s = `<https://coldstart.example/s${i}>`;
  const p = "<https://coldstart.example/p>";
  return [
    `${s} ${p} <https://coldstart.example/o${i}> .`,
    `${s} <https://coldstart.example/score> "${i}"^^<http://www.w3.org/2001/XMLSchema#integer> .`,
    i < SUBJECTS - 1
      ? `${s} <https://coldstart.example/link> <https://coldstart.example/s${i + 1}> .`
      : `${s} <https://coldstart.example/name> "tail" .`,
  ];
}).flat();
const FIRST_QUERY = "PREFIX ex: <https://coldstart.example/> SELECT ?o WHERE { ex:s1 ex:p ?o }";

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

if (process.argv.includes(CHILD_FLAG)) {
  // Fresh realm: exactly one init. Measure entry -> ready -> first query.
  const { performance } = await import("node:perf_hooks");
  const t0 = performance.now();
  const { ready, Dataset, QueryEngine } = await import("../index.mjs");
  await ready();
  const tReady = performance.now();
  const dataset = Dataset.parse(CORPUS.join("\n"), "ntriples");
  if (dataset.size !== SUBJECTS * 3) {
    throw new Error(`corpus size mismatch: parsed ${dataset.size}, expected ${SUBJECTS * 3}`);
  }
  const engine = new QueryEngine();
  const result = engine.select(dataset, FIRST_QUERY);
  const t1 = performance.now();
  if (result.rowCount !== 1) {
    throw new Error(`first query row mismatch: got ${result.rowCount}, expected 1`);
  }
  result.free();
  dataset.free();
  const round3 = (x) => Number((x).toFixed(3));
  process.stdout.write(
    `COLDSTART_SAMPLE ${JSON.stringify({
      ready_ms: round3(tReady - t0),
      first_query_ms: round3(t1 - tReady),
      cold_ms: round3(t1 - t0),
    })}\n`,
  );
} else {
  // Parent: N fresh realms, one sample each.
  try {
    accessSync(PKG_WASM);
  } catch {
    throw new Error(
      `the compiled package is missing (${PKG_WASM}); ` +
        "build it with make wasm-pkg before running this bench",
    );
  }
  const INITS = Number(process.env.BENCH_COLDSTART_INITS ?? 20);
  if (!Number.isInteger(INITS) || INITS < 1) {
    throw new Error(`BENCH_COLDSTART_INITS must be an integer >= 1, got ${INITS}`);
  }
  const samples = [];
  for (let i = 0; i < INITS; i++) {
    const child = spawnSync(process.execPath, [fileURLToPath(import.meta.url), CHILD_FLAG], {
      cwd: BENCH_ROOT,
      encoding: "utf8",
    });
    if (child.status !== 0 || child.error) {
      throw new Error(
        `cold-start child ${i} failed (exit ${child.status ?? child.error}):\n${child.stderr}`,
      );
    }
    const line = child.stdout
      .split("\n")
      .find((l) => l.startsWith("COLDSTART_SAMPLE "));
    if (!line) {
      throw new Error(`cold-start child ${i} printed no COLDSTART_SAMPLE line:\n${child.stdout}`);
    }
    samples.push(JSON.parse(line.slice("COLDSTART_SAMPLE ".length)));
  }

  const pick = (key) => samples.map((s) => s[key]);
  const round3 = (x) => Number(x.toFixed(3));
  const cold = pick("cold_ms");
  const readyMs = pick("ready_ms");
  const firstQueryMs = pick("first_query_ms");
  const fmt = (x, d = 2) => x.toFixed(d);
  // Compact, greppable summary line (grep 'BENCH coldstart').
  console.log(
    `BENCH coldstart: inits=${INITS} ` +
      `median_cold_ms=${fmt(median(cold))} min_cold_ms=${fmt(Math.min(...cold))} ` +
      `max_cold_ms=${fmt(Math.max(...cold))} ` +
      `median_ready_ms=${fmt(median(readyMs))} ` +
      `median_first_query_ms=${fmt(median(firstQueryMs))}`,
  );

  const { version: packageVersion } = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  );
  const resultDocument = {
    bench: "coldstart",
    package_version: packageVersion,
    generated_at: new Date().toISOString(),
    node: process.version,
    platform: `${process.platform} ${process.arch}`,
    tunables: { inits: INITS, corpus_subjects: SUBJECTS, corpus_quads: SUBJECTS * 3 },
    results: {
      cold_ms: { median: round3(median(cold)), min: Math.min(...cold), max: Math.max(...cold), samples: cold },
      ready_ms: { median: round3(median(readyMs)), min: Math.min(...readyMs), max: Math.max(...readyMs), samples: readyMs },
      first_query_ms: {
        median: round3(median(firstQueryMs)),
        min: Math.min(...firstQueryMs),
        max: Math.max(...firstQueryMs),
        samples: firstQueryMs,
      },
    },
  };
  const persistedPath = await writeResultDocument(resultDocument, parseOutArg(process.argv.slice(2)));
  if (persistedPath) console.log(`BENCH coldstart persisted: ${persistedPath}`);
}
