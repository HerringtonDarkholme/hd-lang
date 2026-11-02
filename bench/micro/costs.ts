// One-operation string and collection costs for issue #1208.
//
// Usage:
//   node --experimental-strip-types bench/micro/costs.ts [CASE...]
//   HD_MICRO_RUNS=1 node --experimental-strip-types bench/micro/costs.ts string-trim

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { createHost } from "../../host/node.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const HD = join(ROOT, "target", "release", "hd");
const ALL_CASES = [
  "string-add-build",
  "string-add-assign-build",
  "string-builder-push",
  "string-slice",
  "string-split",
  "string-trim",
  "string-eq-short",
  "string-eq-long",
  "map-get-key-string",
  "string-chars",
  "string-format-int",
  "list-get-i32",
  "list-get-string",
  "map-get-key-i32",
  "list-contains-i32",
  "list-contains-string",
  "map-contains-key-i32",
  "map-contains-key-string",
  "list-for-value-i32",
  "list-for-value-string",
  "list-for-borrow-i32",
  "list-for-borrow-string",
  "map-for-key-i32",
  "map-for-key-string",
  "map-keys-i32",
  "map-keys-string",
  "map-add-key-i32",
  "map-add-key-string",
  "list-push-i32",
  "list-push-string",
  "list-pop-i32",
  "list-pop-string",
  "list-field-write",
  "list-ref-i32",
  "list-ref-string",
  "list-mut-i32",
  "list-mut-string",
  "map-get-or-insert-key-i32",
  "map-get-or-insert-key-string",
  "list-sort-i32",
  "list-sort-string",
  "list-filter-i32",
  "list-filter-string",
  "list-nested",
  "map-nested",
] as const;
const RUNS = Number(process.env.HD_MICRO_RUNS ?? "7");

if (!Number.isInteger(RUNS) || RUNS < 1) {
  throw new Error("HD_MICRO_RUNS must be a positive integer");
}

const requested = process.argv.slice(2);
const unknown = requested.filter((name) => !ALL_CASES.includes(name as (typeof ALL_CASES)[number]));
if (unknown.length > 0) {
  throw new Error(`unknown case: ${unknown.join(", ")}`);
}
const cases = requested.length > 0 ? requested : [...ALL_CASES];

function hdBinary(): string {
  if (!existsSync(HD)) {
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

function makeScratch(name: string): string {
  const file = name.replaceAll("-", "_");
  const scratch = mkdtempSync(join(tmpdir(), "hd-cost-"));
  writeFileSync(join(scratch, "hd.toml"), '[package]\nname = "cost"\n');
  mkdirSync(join(scratch, "src"));
  copyFileSync(join(HERE, `${file}.hd`), join(scratch, "src", "main.hd"));
  return scratch;
}

function buildRelease(hd: string, scratch: string): { bytes: Buffer; size: number } {
  const built = spawnSync(hd, ["build", "--release"], { cwd: scratch, encoding: "utf8" });
  if (built.status !== 0) {
    throw new Error(`hd build --release ${scratch} failed:\n${built.stderr}${built.stdout}`);
  }
  const path = join(scratch, "build", "release", "cost.wasm");
  return { bytes: readFileSync(path), size: statSync(path).size };
}

async function timeHd(wasm: Buffer): Promise<number> {
  const module = await WebAssembly.compile(new Uint8Array(wasm));
  const times: number[] = [];
  for (let run = 0; run < RUNS; run++) {
    let errors = "";
    const host = createHost(
      { out: () => {}, err: (text: string) => (errors += text) },
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
  return Math.min(...times);
}

function timeJs(name: string): number {
  const result = spawnSync(
    "node",
    [
      "--experimental-strip-types",
      join(HERE, "cost-js-runner.ts"),
      join(HERE, `${name}.js`),
      String(RUNS),
    ],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(`node ${name}.js failed:\n${result.stderr}${result.stdout}`);
  }
  return Number(result.stdout.trim());
}

const hd = hdBinary();
const rows: string[][] = [];
for (const name of cases) {
  const scratch = makeScratch(name);
  try {
    const { bytes, size } = buildRelease(hd, scratch);
    const hdMs = await timeHd(bytes);
    const jsMs = timeJs(name);
    rows.push([name, hdMs.toFixed(1), jsMs.toFixed(1), (hdMs / jsMs).toFixed(2), String(size)]);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const header = ["case", "hd (ms)", "js (ms)", "hd/js", "wasm (B)"];
const widths = header.map((cell, column) =>
  Math.max(cell.length, ...rows.map((row) => row[column]!.length)),
);
const line = (cells: readonly string[]): string =>
  cells.map((cell, column) => cell.padStart(widths[column]!)).join("  ");
console.log(line(header));
console.log(widths.map((width) => "-".repeat(width)).join("  "));
for (const row of rows) {
  console.log(line(row));
}
