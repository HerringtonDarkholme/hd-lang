// Runtime microbenchmarks: the same small program in hd, Python, and Node.
//
// Usage: node --experimental-strip-types test/perf/micro/run.ts
//
// Each hd program is first built with `hd build --release` in a scratch
// package, which reports the release Wasm size. Timing then runs the
// program's `main` three times in this process (compile time excluded) and
// takes the median. The Python and Node programs time themselves the same
// way and print their median; each runs in its own process. Python is
// optional: its column is skipped when `python3` is missing. Every program
// checks its own result, so dead-code elimination cannot skip the work.

import {
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
  mkdirSync,
  copyFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { compileToWasm, instantiate } from "../../../src/compiler.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..");
const HD = join(ROOT, "bin", "hd.js");
const CASES = ["fib", "sum", "string-build", "map", "sort"] as const;
const RUNS = 3;

function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)]!;
}

/** Builds the hd program of a case with `hd build --release` in a scratch
 * package and returns the size of the emitted Wasm file. hd module paths
 * are identifiers, so a case's hd file spells `-` as `_`. */
function buildRelease(name: string): number {
  const file = name.replaceAll("-", "_");
  const scratch = mkdtempSync(join(tmpdir(), "hd-micro-"));
  try {
    writeFileSync(join(scratch, "hd.toml"), '[package]\nname = "bench"\n');
    mkdirSync(join(scratch, "src"));
    copyFileSync(join(HERE, `${file}.hd`), join(scratch, "src", `${file}.hd`));
    const build = spawnSync(
      process.execPath,
      ["--experimental-strip-types", HD, "build", "--release", join("src", `${file}.hd`)],
      { cwd: scratch, encoding: "utf8" },
    );
    if (build.status !== 0)
      throw new Error(`hd build --release ${file}.hd failed:\n${build.stderr}${build.stdout}`);
    return statSync(join(scratch, `${file}.wasm`)).size;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

/** Median of RUNS runs of the program's entry point, compile excluded. */
async function timeHd(name: string): Promise<number> {
  const source = readFileSync(join(HERE, `${name.replaceAll("-", "_")}.hd`), "utf8");
  const compilation = await compileToWasm(source, { release: true });
  const times: number[] = [];
  for (let run = 0; run < RUNS; run++) {
    const { instance } = await instantiate(source, { compilation });
    const main = instance.exports.main as CallableFunction;
    const start = performance.now();
    main();
    times.push(performance.now() - start);
  }
  return median(times);
}

/** Runs a self-timing Python or Node program; its stdout is the median ms. */
function timeTool(tool: string, name: string): number | undefined {
  const extension = tool === "python3" ? "py" : "js";
  const result = spawnSync(tool, [join(HERE, `${name}.${extension}`)], { encoding: "utf8" });
  if (result.error) return undefined; // the tool is not installed
  if (result.status !== 0)
    throw new Error(`${tool} ${name}.${extension} failed:\n${result.stderr}${result.stdout}`);
  return Number(result.stdout.trim());
}

const tools = [
  { label: "hd", time: timeHd },
  { label: "python", time: (name: string) => timeTool("python3", name) },
  { label: "node", time: (name: string) => timeTool("node", name) },
] as const;

const rows: string[][] = [];
for (const name of CASES) {
  const wasm = buildRelease(name);
  const cells: string[] = [name];
  for (const tool of tools) {
    const ms = await tool.time(name);
    cells.push(ms === undefined ? "(missing)" : ms.toFixed(1));
  }
  cells.push(String(wasm));
  rows.push(cells);
}

const header = ["case", ...tools.map((tool) => `${tool.label} (ms)`), "wasm (B)"];
const widths = header.map((_, column) =>
  Math.max(header[column]!.length, ...rows.map((row) => row[column]!.length)),
);
const line = (cells: readonly string[]): string =>
  cells.map((cell, column) => cell.padStart(widths[column]!)).join("  ");
console.log(line(header));
console.log(widths.map((width) => "-".repeat(width)).join("  "));
for (const row of rows) console.log(line(row));
