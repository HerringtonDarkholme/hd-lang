// 5.2/5.3: save WAT excerpts for the alloc and erasure probes (user code only).
//   node --experimental-strip-types audit/scripts/arch/erasure-excerpts.ts
import { writeFileSync } from "node:fs";
import { excerpt, readProbe, ROOT, watFor } from "./alloc-lib.ts";

const probes = [
  "audit/probes/arch/alloc/alloc-loops.hd",
  "audit/probes/arch/alloc/alloc-frames.hd",
  "audit/probes/arch/erasure/erasure-bench.hd",
  "audit/probes/arch/erasure/erasure-shared-storage.hd",
];
const date = new Date().toISOString();
for (const probe of probes) {
  const name = probe.split("/").pop()!.replace(/\.hd$/, ".wat");
  writeFileSync(
    `${ROOT}audit/evidence/05-object-model/wat/${name}`,
    `;; commit bd985d7; command: erasure-excerpts.ts (emitWat of ${probe}); date: ${date}\n` +
      excerpt(watFor(readProbe(probe))) +
      "\n",
  );
  console.log(name);
}
