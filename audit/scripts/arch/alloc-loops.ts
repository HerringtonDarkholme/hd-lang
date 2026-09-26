// 5.2: runtime allocations per loop iteration, via counter-instrumented WAT.
// Slope = (count(n=2N) - count(n=N)) / N, so one-time setup cancels.
//   node --experimental-strip-types audit/scripts/arch/alloc-loops.ts [probe.hd]
import { writeFileSync } from "node:fs";
import {
  callExport,
  instantiateCounted,
  readProbe,
  ROOT,
} from "./alloc-lib.ts";

const probe = process.argv[2] ?? "audit/probes/arch/alloc/alloc-loops.hd";
const counted = await instantiateCounted(readProbe(probe));
const provider = { requirement: "Clock" };
const lines: string[] = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/alloc-loops.ts ${probe}; date: ${new Date().toISOString()}`,
  "loop\tallocs/iteration\tby type (per iteration)",
];
const N = 100;
const exports = Object.keys(counted.instance.exports).filter((name) =>
  name.startsWith("loop_"),
);
for (const name of exports) {
  const entry = counted.instance.exports[name] as (
    ...args: unknown[]
  ) => number;
  const needsProvider = entry.length > 1;
  const run = (n: number): Record<string, number> => {
    counted.reset();
    if (needsProvider) entry(n, provider);
    else callExport(counted.instance, name, n);
    return counted.counts();
  };
  const small = run(N);
  const large = run(2 * N);
  const types = new Set([...Object.keys(small), ...Object.keys(large)]);
  let total = 0;
  const parts: string[] = [];
  for (const type of [...types].sort()) {
    const slope = ((large[type] ?? 0) - (small[type] ?? 0)) / N;
    total += slope;
    if (slope !== 0) parts.push(`${type}=${slope}`);
  }
  lines.push(`${name}\t${total}\t${parts.join(" ")}`);
}
const out = `${ROOT}audit/evidence/05-object-model/alloc-per-iteration.tsv`;
writeFileSync(out, lines.join("\n") + "\n");
console.log(lines.join("\n"));
