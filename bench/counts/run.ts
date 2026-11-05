// Counts are deterministic work totals, not a timing benchmark.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compareCounts, countTable, readBaseline, recordCounts } from "./baseline.ts";
import type { Entry } from "./baseline.ts";

const root = fileURLToPath(new URL("../../", import.meta.url));
const options = new Map<string, string>();
const flags = new Set<string>();
const arguments_ = process.argv.slice(2);
for (let index = 0; index < arguments_.length; index++) {
  const argument = arguments_[index];
  if (["--record", "--refresh"].includes(argument)) {
    flags.add(argument);
  } else if (
    ["--compiler", "--baseline", "--previous", "--output", "--only", "--reason"].includes(
      argument,
    ) &&
    arguments_[index + 1]
  ) {
    options.set(argument, arguments_[++index]);
  } else {
    throw new Error(`unknown/missing count gate argument: ${argument}`);
  }
}
const baselinePath = resolve(options.get("--baseline") ?? `${root}/bench/counts/baseline.json`);
const recorded = existsSync(baselinePath)
  ? readBaseline(readFileSync(baselinePath, "utf8"))
  : undefined;
const previousPath = options.get("--previous");
const previous =
  previousPath && existsSync(previousPath)
    ? readBaseline(readFileSync(previousPath, "utf8"))
    : undefined;
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
if (selected && (flags.has("--record") || flags.has("--refresh"))) {
  throw new Error("baseline updates require all benchmark programs");
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
const actual: Entry[] = [];
for (const line of manifest.trim().split("\n")) {
  const [program, path] = line.split("\t");
  process.stderr.write(`count operations: ${program}\n`);
  const text = execFileSync(process.execPath, [`${root}/bench/counts/measure.ts`, program, path], {
    cwd: root,
    encoding: "utf8",
    timeout: 60_000,
    maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, HD_CACHE: `${output}/cache` },
  });
  actual.push(JSON.parse(text) as Entry);
}
const measured = recordCounts(actual);
writeFileSync(`${output}/actual.json`, JSON.stringify(measured, null, 2) + "\n");
const limits =
  selected && recorded
    ? { ...recorded, programs: recorded.programs.filter((row) => selected.includes(row.program)) }
    : recorded;
const table = countTable(actual, previous ?? limits);
writeFileSync(`${output}/summary.md`, table);
process.stdout.write(table);
if (flags.has("--record")) {
  writeFileSync(
    baselinePath,
    JSON.stringify(recordCounts(actual, recorded, options.get("--reason")), null, 2) + "\n",
  );
} else {
  if (!limits) {
    throw new Error("missing baseline; seed it with --record");
  }
  const failures = compareCounts(actual, limits, previous);
  if (failures.length) {
    throw new Error(failures.join("\n"));
  }
  if (flags.has("--refresh")) {
    writeFileSync(baselinePath, JSON.stringify(recordCounts(actual, recorded), null, 2) + "\n");
  }
}
