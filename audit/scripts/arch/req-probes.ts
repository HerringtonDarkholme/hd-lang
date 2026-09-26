// 5.4 requirement passing: generate chain probes, count allocations in WAT,
// instrument the pack helpers, and time per-call cost.
// Run: node --experimental-strip-types audit/scripts/arch/req-probes.ts
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { assembleWat } from "../../../src/wasm.ts";
import {
  ALLOCATION_PATTERN,
  compilePhased,
  countMatches,
  exported,
  functionBodies,
  header,
  instantiateWithBytes,
  median,
  spread,
  timeCalls,
} from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const probeDir = resolve(root, "audit/probes/arch/requirements");
const evidenceDir = resolve(root, "audit/evidence/05-requirements");
const ITERATIONS = 200_000;
const DEPTH = 10;

function traits(count: number): string {
  return Array.from(
    { length: count },
    (_, index) =>
      `trait R${index + 1}:\n    fn v(self) -> i32\n\ndata P${index + 1}:\n    x: i32\n\nimpl R${index + 1} for P${index + 1}:\n    fn v(self) -> i32: self.x\n`,
  ).join("\n");
}

function row(count: number): string {
  return Array.from({ length: count }, (_, index) => `R${index + 1}`).join(" + ");
}

function withAll(count: number): string {
  return Array.from({ length: count }, (_, index) => `R${index + 1}=P${index + 1} { x: 1 }`).join(
    ", ",
  );
}

// Concrete rows: a 10-deep chain whose every link declares the same row.
export function concreteChain(count: number): string {
  const suffix = count > 0 ? ` $ ${row(count)}` : "";
  const leafBody = count > 0 ? "$.use(R1).v()" : "1";
  const links = [`fn c${DEPTH}(n: i32) -> i32${suffix}:\n    n + ${leafBody}\n`];
  for (let level = DEPTH - 1; level >= 1; level -= 1)
    links.push(`fn c${level}(n: i32) -> i32${suffix}:\n    c${level + 1}(n)\n`);
  const loop = `let index: i32 = 0\n    let total: i32 = 0\n    while index < ${ITERATIONS}:\n        total = (total + c1(index % 7)) % 1000003\n        index = index + 1\n    total`;
  const body =
    count > 0
      ? `fn main() -> i32:\n    $.with(${withAll(count)}):\n        ${loop.replaceAll("\n    ", "\n        ")}\n`
      : `fn main() -> i32:\n    ${loop}\n`;
  return `# probe 5.4: 10-deep concrete-row chain carrying ${count} requirement(s)\n${traits(count)}\n${links.join("\n")}\n${body}`;
}

// Generic rows: the chain forwards a callback whose row is a row variable, so
// every link passes one keyed pack and the callback adapter looks up each key.
export function rowChain(count: number): string {
  const links = [`fn g${DEPTH}[r](cb: fn(i32) -> i32 $ r, n: i32) -> i32 $ r:\n    cb(n)\n`];
  for (let level = DEPTH - 1; level >= 1; level -= 1)
    links.push(`fn g${level}[r](cb: fn(i32) -> i32 $ r, n: i32) -> i32 $ r:\n    g${level + 1}(cb, n)\n`);
  const uses = Array.from({ length: count }, (_, index) => `$.use(R${index + 1}).v()`).join(" + ");
  const leaf = `fn leaf(n: i32) -> i32 $ ${row(count)}:\n    n + ${uses}\n`;
  const loop = `let index: i32 = 0\n        let total: i32 = 0\n        while index < ${ITERATIONS}:\n            total = (total + g1(leaf, index % 7)) % 1000003\n            index = index + 1\n        total`;
  return `# probe 5.4: 10-deep row-generic chain; callback row has ${count} requirement(s)\n${traits(count)}\n${leaf}\n${links.join("\n")}\nfn main() -> i32:\n    $.with(${withAll(count)}):\n        ${loop}\n`;
}

function instrument(wat: string): string {
  const counters = `  (global $audit.concat (export "audit_concat") (mut i32) (i32.const 0))\n  (global $audit.get (export "audit_get") (mut i32) (i32.const 0))\n  (global $audit.node (export "audit_node") (mut i32) (i32.const 0))\n`;
  let out = wat.replace(/\n(\s*\(func \$hd\.provider_get)/, `\n${counters}$1`);
  out = out.replace(
    /(\(func \$hd\.provider_get\s*\n\s*\(param \$providers \(ref null \$hd\.providers\)\)\s*\n\s*\(param \$key i32\)\s*\n\s*\(result anyref\)\s*\n\s*\(local \$cursor \(ref null \$hd\.providers\)\))/,
    `$1\n    (global.set $audit.get (i32.add (global.get $audit.get) (i32.const 1)))`,
  );
  out = out.replace(
    /(\(func \$hd\.provider_concat\s*\n\s*\(param \$left \(ref null \$hd\.providers\)\)\s*\n\s*\(param \$right \(ref null \$hd\.providers\)\)\s*\n\s*\(result \(ref null \$hd\.providers\)\))/,
    `$1\n    (global.set $audit.concat (i32.add (global.get $audit.concat) (i32.const 1)))\n    (if (i32.eqz (ref.is_null (local.get $left))) (then (global.set $audit.node (i32.add (global.get $audit.node) (i32.const 1)))))`,
  );
  if (out === wat || !out.includes("$audit.get (i32.add") || !out.includes("$audit.node (i32.add"))
    throw new Error("instrumentation did not apply");
  return out;
}

interface Row {
  readonly name: string;
  readonly count: number;
  readonly perCallNs: number;
  readonly medianNs: number;
  readonly spread: string;
  readonly allocations: number;
  readonly hiddenParams: string;
  readonly concatPerCall: number;
  readonly nodesPerCall: number;
  readonly getsPerCall: number;
}

async function measure(name: string, count: number, source: string): Promise<Row> {
  writeFileSync(resolve(probeDir, `${name}.hd`), source);
  const compiled = compilePhased(source);
  writeFileSync(
    resolve(evidenceDir, `${name}.wat`),
    `;; commit bd985d7; emitted by audit/scripts/arch/req-probes.ts for ${name}.hd; ${new Date().toISOString()}\n${compiled.wat}`,
  );
  const bodies = functionBodies(compiled.wat);
  const userFunctions = [...bodies].filter(([fn]) => !fn.startsWith("$hd."));
  const allocations = userFunctions.reduce(
    (sum, [, body]) => sum + countMatches(body, ALLOCATION_PATTERN),
    0,
  );
  const link = bodies.get("$f" + (name.startsWith("row") ? "1" : "0")) ?? "";
  const hiddenParams = String(countMatches(link.split("\n")[0] ?? "", /\(param \$provider/g));
  const instance = await instantiateWithBytes(source, undefined);
  const main = exported(instance, "main");
  const samples = timeCalls(() => main(), 15, 5);
  const instrumented = assembleWat(instrument(compiled.wat));
  const probe = await instantiateWithBytes(source, instrumented.bytes);
  exported(probe, "main")();
  const global = (key: string): number =>
    Number((probe.exports[key] as WebAssembly.Global).value) / ITERATIONS;
  return {
    name,
    count,
    perCallNs: (Math.min(...samples) * 1e6) / ITERATIONS,
    medianNs: (median(samples) * 1e6) / ITERATIONS,
    spread: spread(samples),
    allocations,
    hiddenParams,
    concatPerCall: global("audit_concat"),
    nodesPerCall: global("audit_node"),
    getsPerCall: global("audit_get"),
  };
}

const rows: Row[] = [];
for (const count of [0, 1, 5, 10]) rows.push(await measure(`chain-${count}`, count, concreteChain(count)));
for (const count of [1, 5, 10]) rows.push(await measure(`row-chain-${count}`, count, rowChain(count)));

const table = [
  "| probe | reqs | ns per 10-deep call, min / median of 15 | sample ms range | static allocs in user fns | hidden provider params on a link | concat calls/call | pack nodes copied/call | pack lookups/call |",
  "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  ...rows.map(
    (row) =>
      `| ${row.name} | ${row.count} | ${row.perCallNs.toFixed(1)} / ${row.medianNs.toFixed(1)} | ${row.spread} | ${row.allocations} | ${row.hiddenParams} | ${row.concatPerCall} | ${row.nodesPerCall} | ${row.getsPerCall} |`,
  ),
].join("\n");
writeFileSync(
  resolve(evidenceDir, "req-scaling.md"),
  header("node --experimental-strip-types audit/scripts/arch/req-probes.ts") +
    `\nIterations per sample: ${ITERATIONS}. Hidden-param column reads the first chain link's export signature (for row-chain probes that function is the exported \`leaf\`, which takes one param per concrete entry; the ten \`g\` links each take a single \`$hd.providers\` pack parameter).\n\n${table}\n`,
);
console.log(table);
