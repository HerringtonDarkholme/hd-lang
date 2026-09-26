// 5.6: how much of Binaryen assembly time the always-inlined runtime costs.
// Compares assembling the minimal program's module with a hand-written module
// of the same observable behavior.
// Run: node --experimental-strip-types audit/scripts/arch/rtlib-assemble.ts
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { assembleWat } from "../../../src/wasm.ts";
import { compilePhased, header, median } from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const source = readFileSync(resolve(root, "audit/probes/arch/runtime-lib/minimal.hd"), "utf8");
const tiny = '(module (func (export "main") (result i32) (i32.const 0)))';
const emitted: number[] = [];
const hand: number[] = [];
for (let index = 0; index < 30; index += 1) {
  emitted.push(compilePhased(source).times.assemble);
  const start = performance.now();
  assembleWat(tiny);
  hand.push(performance.now() - start);
}
const text = [
  header("node --experimental-strip-types audit/scripts/arch/rtlib-assemble.ts"),
  "| module | assembleWat ms, min / median of 30 |",
  "| --- | --- |",
  `| emitted for \`fn main() -> i32: 0\` | ${Math.min(...emitted).toFixed(2)} / ${median(emitted).toFixed(2)} |`,
  `| hand-written equivalent | ${Math.min(...hand).toFixed(2)} / ${median(hand).toFixed(2)} |`,
].join("\n");
writeFileSync(resolve(root, "audit/evidence/05-requirements/rtlib-assemble.md"), text + "\n");
console.log(text);
