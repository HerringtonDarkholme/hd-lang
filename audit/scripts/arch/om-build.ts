// 5.1: build every object-model probe, save WAT excerpts, and tally allocation
// sites per user function. Usage:
//   node --experimental-strip-types audit/scripts/arch/om-build.ts
import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import {
  allocationSites,
  excerpt,
  isRuntimeFunction,
  readProbe,
  ROOT,
  watFor,
} from "./alloc-lib.ts";

const probeDir = "audit/probes/arch/object-model/";
const outDir = `${ROOT}audit/evidence/05-object-model/wat/`;
mkdirSync(outDir, { recursive: true });
const date = new Date().toISOString();
const lines: string[] = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/om-build.ts; date: ${date}`,
  "probe\tfunction\tallocation sites (op type)",
];
for (const file of readdirSync(ROOT + probeDir)
  .filter((name) => name.endsWith(".hd"))
  .sort()) {
  const source = readProbe(probeDir + file);
  let wat: string;
  try {
    wat = watFor(source);
  } catch (error) {
    lines.push(`${file}\t-\tCHECK FAILED ${String(error).split("\n")[0]}`);
    continue;
  }
  let body: string;
  try {
    body = excerpt(wat);
  } catch (error) {
    const functions = wat.slice(wat.indexOf("(elem declare"));
    writeFileSync(
      `${outDir}${file.replace(/\.hd$/, ".wat")}`,
      `;; UNBALANCED WAT (${String(error)}); tail of emitted module:\n${functions}`,
    );
    lines.push(`${file}\t-\tUNBALANCED WAT: ${String(error)}`);
    continue;
  }
  writeFileSync(
    `${outDir}${file.replace(/\.hd$/, ".wat")}`,
    `;; commit bd985d7; command: om-build.ts (emitWat of ${probeDir}${file}); date: ${date}\n;; excerpt: rec types, user globals, user functions (runtime $hd.* functions omitted)\n` +
      body +
      "\n",
  );
  const byFunction = new Map<string, string[]>();
  for (const site of allocationSites(wat)) {
    if (isRuntimeFunction(site.func)) continue;
    byFunction.set(site.func, [
      ...(byFunction.get(site.func) ?? []),
      `${site.op} ${site.type}`,
    ]);
  }
  for (const [func, sites] of byFunction)
    lines.push(`${file}\t${func}\t${sites.join(", ")}`);
}
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/static-alloc-sites.tsv`,
  lines.join("\n") + "\n",
);
console.log(lines.join("\n"));
