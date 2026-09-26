// 5.7 direction benchmarks: size, compile time, and run time for dev and
// binaryen -O2 builds of the four-program set in audit/bench/.
// Run: node --experimental-strip-types audit/scripts/arch/bench-run.ts [--quick]
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import {
  compilePhased,
  exported,
  header,
  instantiateWithBytes,
  median,
  optimize,
  optimizedText,
  spread,
  timeCalls,
} from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const benchDir = resolve(root, "audit/bench");
const quick = process.argv.includes("--quick");
const SAMPLES = quick ? 3 : 21;
const COMPILES = quick ? 2 : 5;
const CHAIN_ITERATIONS = 2_000_000;

// Bench 3 sources: 10-deep concrete-row chain carrying 1 or 5 requirements.
function chain(count: number): string {
  const traits = Array.from(
    { length: count },
    (_, index) =>
      `trait R${index + 1}:\n    fn v(self) -> i32\n\ndata P${index + 1}:\n    x: i32\n\nimpl R${index + 1} for P${index + 1}:\n    fn v(self) -> i32: self.x\n`,
  ).join("\n");
  const row = Array.from({ length: count }, (_, index) => `R${index + 1}`).join(" + ");
  const links = [`fn c10(n: i32) -> i32 $ ${row}:\n    n + $.use(R1).v()\n`];
  for (let level = 9; level >= 1; level -= 1)
    links.push(`fn c${level}(n: i32) -> i32 $ ${row}:\n    c${level + 1}(n)\n`);
  const providers = Array.from({ length: count }, (_, index) => `R${index + 1}=P${index + 1} { x: 1 }`).join(", ");
  return `# bench 5.7-3: 10-deep call chain carrying ${count} requirement(s); leaf uses R1\n${traits}\n${links.join("\n")}\nfn main() -> i32:\n    $.with(${providers}):\n        let index: i32 = 0\n        let total: i32 = 0\n        while index < ${CHAIN_ITERATIONS}:\n            total = (total + c1(index % 7)) % 1000003\n            index = index + 1\n        total\n`;
}
writeFileSync(resolve(benchDir, "b3-req1.hd"), chain(1));
writeFileSync(resolve(benchDir, "b3-req5.hd"), chain(5));

interface Result {
  readonly name: string;
  readonly devBytes: number;
  readonly o2Bytes: number;
  readonly compileMs: number;
  readonly phases: string;
  readonly o2Ms: number;
  readonly devRun: number[];
  readonly o2Run: number[];
  readonly stubRun?: number[];
  readonly value: string;
}

// Replace the development runtime's pending/trace hooks with constant stubs, to
// separate in-Wasm suspension cost from the JS hook cost.
async function withStubHooks<T>(action: () => Promise<T>): Promise<T> {
  const original = WebAssembly.instantiate;
  (WebAssembly as { instantiate: unknown }).instantiate = (bytes: BufferSource, imports: WebAssembly.Imports) =>
    original(bytes, { ...imports, hd: { ...imports.hd, pending: () => 0, trace: () => undefined } });
  try {
    return await action();
  } finally {
    (WebAssembly as { instantiate: unknown }).instantiate = original;
  }
}

async function bench(name: string): Promise<Result> {
  const source = readFileSync(resolve(benchDir, `${name}.hd`), "utf8");
  const compiles = Array.from({ length: COMPILES }, () => compilePhased(source));
  const compiled = compiles.at(-1)!;
  const phaseKeys = ["parse", "check", "emit", "assemble"] as const;
  const phases = phaseKeys
    .map((key) => `${key} ${median(compiles.map((c) => c.times[key])).toFixed(1)}`)
    .join(" / ");
  const o2Start = performance.now();
  const o2 = optimize(compiled.wat);
  const o2Ms = performance.now() - o2Start;
  writeFileSync(resolve(benchDir, `${name}.O2.wat`), `;; commit bd985d7; binaryen -O2 of ${name}.hd; ${new Date().toISOString()}\n${optimizedText(compiled.wat)}`);
  writeFileSync(resolve(benchDir, `${name}.dev.wat`), `;; commit bd985d7; hd build --wat ${name}.hd; ${new Date().toISOString()}\n${compiled.wat}`);
  const devInstance = await instantiateWithBytes(source, compiled.bytes);
  const o2Instance = await instantiateWithBytes(source, o2);
  const devMain = exported(devInstance, "main");
  const o2Main = exported(o2Instance, "main");
  const value = String(devMain());
  if (String(o2Main()) !== value) throw new Error(`${name}: -O2 result differs`);
  // Interleave dev and -O2 samples so contention hits both alike.
  const devRun: number[] = [];
  const o2Run: number[] = [];
  timeCalls(() => devMain(), 0, 5);
  timeCalls(() => o2Main(), 0, 5);
  for (let sample = 0; sample < SAMPLES; sample += 1) {
    devRun.push(...timeCalls(() => devMain(), 1, 0));
    o2Run.push(...timeCalls(() => o2Main(), 1, 0));
  }
  let stubRun: number[] | undefined;
  if (compiled.wat.includes('(import "hd" "pending"')) {
    const stubInstance = await withStubHooks(() => instantiateWithBytes(source, compiled.bytes));
    const stubMain = exported(stubInstance, "main");
    stubRun = timeCalls(() => stubMain(), SAMPLES, 5);
  }
  return {
    name,
    devBytes: compiled.bytes.length,
    o2Bytes: o2.length,
    compileMs: median(compiles.map((c) => c.times.total)),
    phases,
    o2Ms,
    devRun,
    o2Run,
    stubRun,
    value,
  };
}

const names = ["b1-scalar", "b2-generic", "b2-mono", "b3-req1", "b3-req5", "b4-suspend", "b4-plain"];
const results = new Map<string, Result>();
for (const name of names) results.set(name, await bench(name));

const m = (values: readonly number[]): number => median(values);
const lo = (values: readonly number[]): number => Math.min(...values);
const lines = [
  header(`node --experimental-strip-types audit/scripts/arch/bench-run.ts${quick ? " --quick" : ""}`),
  `Samples: ${SAMPLES} interleaved dev/-O2 calls of \`main\` per program after 5 warmups; ${COMPILES} compiles. CPU shared with other audit workers: read ratios, not absolutes. Min is the least-contended sample and is the primary estimator; load average at start: ${(await import("node:os")).loadavg()[0]!.toFixed(1)} on ${(await import("node:os")).availableParallelism()} cores.`,
  "",
  "| program | result | dev wasm B | -O2 wasm B | compile ms (median) | phases ms parse / check / emit / assemble | -O2 pass ms | dev run ms min / median (range) | -O2 run ms min / median (range) | dev/-O2 run (min) |",
  "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  ...[...results.values()].map(
    (r) =>
      `| ${r.name} | ${r.value} | ${r.devBytes} | ${r.o2Bytes} | ${r.compileMs.toFixed(1)} | ${r.phases} | ${r.o2Ms.toFixed(1)} | ${lo(r.devRun).toFixed(2)} / ${m(r.devRun).toFixed(2)} (${spread(r.devRun)}) | ${lo(r.o2Run).toFixed(2)} / ${m(r.o2Run).toFixed(2)} (${spread(r.o2Run)}) | ${(lo(r.devRun) / lo(r.o2Run)).toFixed(2)} |`,
  ),
  "",
  "| pair | dev ratio (min) | -O2 ratio (min) | dev ratio (median) | -O2 ratio (median) | size ratio dev |",
  "| --- | --- | --- | --- | --- | --- |",
];
const pair = (label: string, a: string, b: string) => {
  const left = results.get(a)!;
  const right = results.get(b)!;
  lines.push(
    `| ${label} (${a} / ${b}) | ${(lo(left.devRun) / lo(right.devRun)).toFixed(2)} | ${(lo(left.o2Run) / lo(right.o2Run)).toFixed(2)} | ${(m(left.devRun) / m(right.devRun)).toFixed(2)} | ${(m(left.o2Run) / m(right.o2Run)).toFixed(2)} | ${(left.devBytes / right.devBytes).toFixed(2)} |`,
  );
};
pair("erased / monomorphic", "b2-generic", "b2-mono");
pair("5-row / 1-row", "b3-req5", "b3-req1");
pair("suspending / plain", "b4-suspend", "b4-plain");
const suspend = results.get("b4-suspend")!;
if (suspend.stubRun)
  lines.push(
    "",
    `b4-suspend with the dev runtime's \`pending\`/\`trace\` imports replaced by constant stubs: min ${lo(suspend.stubRun).toFixed(2)} ms, median ${m(suspend.stubRun).toFixed(2)} ms (${spread(suspend.stubRun)}); min ratio to b4-plain dev: ${(lo(suspend.stubRun) / lo(results.get("b4-plain")!.devRun)).toFixed(2)}; min ratio to b4-suspend dev: ${(lo(suspend.stubRun) / lo(suspend.devRun)).toFixed(2)}.`,
  );
writeFileSync(resolve(root, "audit/evidence/05-requirements/bench.md"), lines.join("\n") + "\n");
console.log(lines.join("\n"));
