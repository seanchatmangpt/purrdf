// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Shared result persistence for the Node benches (the ONE home of the --out
// pattern): parse.bench.mjs and coldstart.bench.mjs both land their full
// result documents here, so the flag parsing and the write/print branch exist
// exactly once. Default (no --out) prints the JSON document to stdout;
// `--out <path>` writes it to that path instead, creating parent directories.
//
// Snapshots are deliberately NOT gitignored — see the committed-snapshot
// pattern documented in parse.bench.mjs's header: a snapshot committed next
// to the change it measured is the baseline a future run diffs against.

import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

export function parseOutArg(argv) {
  const i = argv.indexOf("--out");
  if (i === -1) return null;
  const value = argv[i + 1];
  if (value === undefined || value.startsWith("-")) {
    throw new Error("--out requires a file path argument");
  }
  return value;
}

export async function writeResultDocument(document, outPath) {
  const json = `${JSON.stringify(document, null, 2)}\n`;
  if (outPath) {
    const target = resolve(outPath);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, json, "utf8");
    return target;
  }
  process.stdout.write(json);
  return null;
}
