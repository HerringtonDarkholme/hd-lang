// Counts are deterministic work totals, not a timing benchmark.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compareCounts, gate, readCounts } from "./baseline.ts";
import type { Entry } from "./baseline.ts";
import { readResult, resultKey, runnerDigest, writeResult } from "./results.ts";

const root = fileURLToPath(new URL("../../", import.meta.url));
const options = new Map<string, string>();
const flags = new Set<string>();
const arguments_ = process.argv.slice(2);
for (let index = 0; index < arguments_.length; index++) {
  const argument = arguments_[index];
  if (argument === "--no-cache") {
    flags.add(argument);
  } else if (
    ["--compiler", "--base", "--cost-ok", "--output", "--only"].includes(argument) &&
    arguments_[index + 1]
  ) {
    options.set(argument, arguments_[++index]);
  } else {
    throw new Error(`unknown/missing count gate argument: ${argument}`);
  }
}
const output = resolve(options.get("--output") ?? `${root}/target/count-ops`);
mkdirSync(output, { recursive: true });
const all = ["bench/study", "bench/runtime/progs"]
  .flatMap((directory) =>
    readdirSync(`${root}/${directory}`)
      .filter((name) => name.endsWith(".hd"))
      .map((name) => `${directory}/${name}`),
  )
  .sort();
const selected = options.get("--only")?.split(",");
if (selected?.some((path) => !all.includes(path))) {
  throw new Error("--only must name complete benchmark paths");
}
const programs = selected ? all.filter((path) => selected.includes(path)) : all;
const tool = resolve(
  options.get("--compiler") ??
    `${root}/target/debug/examples/count_ops${process.platform === "win32" ? ".exe" : ""}`,
);
if (!existsSync(tool)) {
  throw new Error("build the tool first: cargo build -p hd_driver --example count_ops");
}
const manifest = execFileSync(tool, [`${output}/modules`, ...programs], {
  cwd: root,
  encoding: "utf8",
  timeout: 600_000,
  env: { ...process.env, HD_CACHE: `${output}/cache` },
});
const reuse = !flags.has("--no-cache");
const results = `${root}/target/count-ops/results`;
const runner = runnerDigest(root);
const actual: Entry[] = [];
let executed = 0;
for (const line of manifest.trim().split("\n")) {
  const [program, path] = line.split("\t");
  const key = resultKey(runner, program, readFileSync(path));
  const stored = reuse ? readResult(results, key, program) : undefined;
  if (stored) {
    actual.push(stored);
    continue;
  }
  process.stderr.write(`count operations: ${program}\n`);
  executed++;
  const text = execFileSync(process.execPath, [`${root}/bench/counts/measure.ts`, program, path], {
    cwd: root,
    encoding: "utf8",
    timeout: 60_000,
    maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, HD_CACHE: `${output}/cache` },
  });
  const row = JSON.parse(text) as Entry;
  writeResult(results, key, row);
  actual.push(row);
}
process.stderr.write(`count operations: ran ${executed} of ${actual.length} programs\n`);
writeFileSync(`${output}/actual.json`, JSON.stringify({ programs: actual }, null, 2) + "\n");
gate(output, options, readCounts, (base) => compareCounts(actual, base));
