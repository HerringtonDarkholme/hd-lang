import fs from "node:fs";
import { performance } from "node:perf_hooks";

function quantile(values, q) {
  const sorted = [...values].sort((a, b) => a - b);
  if (sorted.length === 0) return 0;
  const index = Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1);
  return sorted[Math.max(0, index)];
}

function stats(values) {
  return {
    p10: quantile(values, 0.10),
    p50: quantile(values, 0.50),
    p90: quantile(values, 0.90),
    p95: quantile(values, 0.95),
  };
}

function elapsedMs(action) {
  const start = performance.now();
  const value = action();
  return [performance.now() - start, value];
}

function makeRunner(item, instance) {
  const wasmRun = instance.exports.run;
  if (typeof wasmRun !== "function") throw new Error(`${item.name}: missing run export`);
  const n = item.work;
  if (item.node_kernel === "for-loop") {
    return () => {
      let sum = 0;
      for (let index = 0; index < n; index += 1) sum += index + 1;
      return BigInt(sum);
    };
  }
  if (item.node_kernel?.startsWith("map-")) {
    const stringKeys = item.node_kernel.endsWith("str16");
    const keys = Array.from({ length: n }, (_, index) =>
      stringKeys ? index.toString(16).padStart(16, "0") : BigInt(index),
    );
    if (item.node_kernel.startsWith("map-insert")) {
      return () => {
        const map = new Map();
        for (let index = 0; index < n; index += 1) map.set(keys[index], index);
        return BigInt(map.size);
      };
    }
    const map = new Map(keys.map((key, index) => [key, index]));
    return () => {
      let sum = 0;
      for (let index = 0; index < n; index += 1) sum += map.get(keys[index]);
      return BigInt(sum);
    };
  }
  if (item.node_kernel === "indexof") {
    const bytes = Buffer.alloc(n, 65);
    bytes[n - 1] = 127;
    return () => BigInt(bytes.indexOf(127));
  }
  if (item.node_kernel === "split") {
    const text = `${"alpha ".repeat(Math.ceil(n / 6))}`.slice(0, n);
    return () => BigInt(text.split(" ").length);
  }
  if (item.node_kernel === "equal64") {
    const left = "x".repeat(64);
    const right = "x".repeat(64);
    return () => {
      let equal = 0;
      for (let index = 0; index < n; index += 64) equal += left === right ? 1 : 0;
      return BigInt(equal);
    };
  }
  if (item.host_fill_bytes > 0) {
    const memory = instance.exports.memory;
    const source = new Uint8Array(item.host_fill_bytes).fill(0x5a);
    return () => {
      new Uint8Array(memory.buffer, 0, item.host_fill_bytes).set(source);
      return wasmRun(n);
    };
  }
  if (item.host_read_bytes > 0) {
    const memory = instance.exports.memory;
    return () => {
      const copy = new Uint8Array(memory.buffer, 0, item.host_read_bytes).slice();
      return BigInt(copy[0]) ^ BigInt(wasmRun(n));
    };
  }
  return () => wasmRun(n);
}

const [
  manifestPath,
  mode,
  samplesText,
  warmupsText,
  compileSamplesText,
  compileWarmupsText,
] = process.argv.slice(2);
if (!manifestPath || !mode) {
  throw new Error("usage: node [tier flag] node-runner.mjs MANIFEST MODE SAMPLES WARMUPS");
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const samples = Number(samplesText ?? 15);
const warmups = Number(warmupsText ?? 3);
const compileSamples = Number(compileSamplesText ?? 3);
const compileWarmups = Number(compileWarmupsText ?? 1);
const rows = [];

for (const item of manifest.cases) {
  const bytes = fs.readFileSync(item.path);
  const compile = [];
  for (let index = 0; index < compileWarmups + compileSamples; index += 1) {
    const [ms] = elapsedMs(() => new WebAssembly.Module(bytes));
    if (index >= compileWarmups) compile.push(ms);
  }

  const module = new WebAssembly.Module(bytes);
  const instantiate = [];
  for (let index = 0; index < warmups + samples; index += 1) {
    const [ms] = elapsedMs(() => new WebAssembly.Instance(module, {}));
    if (index >= warmups) instantiate.push(ms);
  }

  const instance = new WebAssembly.Instance(module, {});
  const run = makeRunner(item, instance);
  for (let index = 0; index < warmups; index += 1) run();
  const runtime = [];
  let checksum = 0n;
  for (let index = 0; index < samples; index += 1) {
    const [ms, value] = elapsedMs(run);
    runtime.push(ms);
    checksum ^= typeof value === "bigint" ? value : BigInt(value);
  }

  rows.push({
    name: item.name,
    mode,
    compile_ms: stats(compile),
    instantiate_ms: stats(instantiate),
    runtime_ms: stats(runtime),
    checksum: checksum.toString(),
  });
}

process.stdout.write(JSON.stringify({
  node: process.version,
  v8: process.versions.v8,
  mode,
  rows,
}));
