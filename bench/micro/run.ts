// Runtime microbenchmarks: the same small program in hd, Python, and Node.
//
// Usage: node --experimental-strip-types bench/micro/run.ts
//
// Each hd program is built with the Rust `hd` (`target/release/hd
// build --release`) in a scratch package, which reports the release Wasm
// size; the binary is built with cargo first when it is missing. Timing
// then runs the program's entry point seven times on Node (compile
// excluded) and takes the minimum. The Python and Node programs time
// themselves the same way and print their median; each runs in its own
// process. Python is optional: its column is skipped when `python3` is
// missing. Every program checks its own result, so dead-code elimination
// cannot skip the work.

import {
  existsSync,
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

import { createHost } from "../../host/node.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const HD = join(ROOT, "target", "release", "hd");
const CASES = ["fib", "sum", "string-build", "map", "sort"] as const;
const RUNS = 7;

function min(values: readonly number[]): number {
  return Math.min(...values);
}

/** The Rust `hd` binary, built with cargo when it is missing. */
function hdBinary(): string {
  if (!existsSync(HD)) {
    console.log("building the release hd binary (one time)...");
    const build = spawnSync("cargo", ["build", "--release", "-p", "hd_cli"], {
      cwd: ROOT,
      encoding: "utf8",
    });
    if (build.status !== 0 || !existsSync(HD)) {
      throw new Error(`cargo build --release -p hd_cli failed:\n${build.stderr}${build.stdout}`);
    }
  }
  return HD;
}

/** Scratch package whose default executable is the case's hd program. A
 * case's hd file spells `-` as `_`, since hd module paths are
 * identifiers. Returns the scratch directory. */
function makeScratch(name: string): string {
  const file = name.replaceAll("-", "_");
  const scratch = mkdtempSync(join(tmpdir(), "hd-micro-"));
  writeFileSync(join(scratch, "hd.toml"), '[package]\nname = "bench"\n');
  mkdirSync(join(scratch, "src"));
  copyFileSync(join(HERE, `${file}.hd`), join(scratch, "src", "main.hd"));
  return scratch;
}

/** Builds the case's hd program with `hd build --release` and returns the
 * Wasm bytes and its size. `hd` writes the default executable to
 * build/release/bench.wasm. */
function buildRelease(hd: string, scratch: string): { bytes: Buffer; size: number } {
  const build = spawnSync(hd, ["build", "--release"], { cwd: scratch, encoding: "utf8" });
  if (build.status !== 0) {
    throw new Error(`hd build --release ${scratch} failed:\n${build.stderr}${build.stdout}`);
  }
  const path = join(scratch, "build", "release", "bench.wasm");
  const bytes = readFileSync(path);
  return { bytes: Buffer.from(bytes), size: statSync(path).size };
}

/** Minimum of RUNS runs of the program's entry point on Node, compile
 * excluded: the module compiles once, each run instantiates fresh and
 * times only init-to-completion. */
async function timeHd(wasm: Buffer): Promise<number> {
  const module = await WebAssembly.compile(new Uint8Array(wasm));
  const times: number[] = [];
  for (let run = 0; run < RUNS; run++) {
    let errors = "";
    const host = createHost(
      {
        out: () => {},
        err: (text: string) => {
          errors += text;
        },
      },
      { args: [], program: "", grants: {} },
    );
    const instance = await WebAssembly.instantiate(module, host.imports);
    const start = performance.now();
    const { status } = await host.run(instance, "hd.init", "hd.poll");
    times.push(performance.now() - start);
    host.close();
    if (status !== 0) {
      throw new Error(`hd case exited with status ${status}:\n${errors}`);
    }
  }
  return min(times);
}

/** Runs a self-timing Python or Node program; its stdout is the median ms. */
function timeTool(tool: string, name: string): number | undefined {
  const extension = tool === "python3" ? "py" : "js";
  const result = spawnSync(tool, [join(HERE, `${name}.${extension}`)], { encoding: "utf8" });
  if (result.error) {
    return undefined;
  } // the tool is not installed
  if (result.status !== 0) {
    throw new Error(`${tool} ${name}.${extension} failed:\n${result.stderr}${result.stdout}`);
  }
  return Number(result.stdout.trim());
}

const baselines = [
  { label: "python", time: (name: string) => timeTool("python3", name) },
  { label: "node", time: (name: string) => timeTool("node", name) },
] as const;

const hd = hdBinary();
const rows: string[][] = [];
for (const name of CASES) {
  const scratch = makeScratch(name);
  try {
    const { bytes, size } = buildRelease(hd, scratch);
    const cells: string[] = [name, (await timeHd(bytes)).toFixed(1)];
    for (const tool of baselines) {
      const ms = await tool.time(name);
      cells.push(ms === undefined ? "(missing)" : ms.toFixed(1));
    }
    cells.push(String(size));
    rows.push(cells);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const header = ["case", "hd (ms)", "python (ms)", "node (ms)", "wasm (B)"];
const widths = header.map((_, column) =>
  Math.max(header[column]!.length, ...rows.map((row) => row[column]!.length)),
);
const line = (cells: readonly string[]): string =>
  cells.map((cell, column) => cell.padStart(widths[column]!)).join("  ");
console.log(line(header));
console.log(widths.map((width) => "-".repeat(width)).join("  "));
for (const row of rows) {
  console.log(line(row));
}
