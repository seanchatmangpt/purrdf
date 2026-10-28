import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as root from "../index.mjs";

// Mechanical gate for the class-reexport parity class: every class the
// generated d.ts exports must be importable from the package root. A
// missing mjs re-export (the ProvenanceInfo regression), a duplicate
// binding, or a link-time failure all fail this assert.
const dts = readFileSync(new URL("../index.d.ts", import.meta.url), "utf8");
const classes = [...dts.matchAll(/^export class (\w+)/gm)].map((m) => m[1]);
assert.ok(
  classes.length >= 28,
  `d.ts class scan found only ${classes.length} (parser desync?)`,
);

for (const name of classes) {
  assert.equal(
    typeof root[name],
    "function",
    `index.d.ts exports class ${name} but index.mjs does not re-export it`,
  );
}
