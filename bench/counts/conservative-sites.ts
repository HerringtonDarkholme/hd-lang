// Static conservative-site budget. The Rust corpus reader compiles only benchmarks.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareSites,
  readSiteBudget,
  readSiteReport,
  recordSites,
  siteTable,
} from "./conservative-baseline.ts";

const root = fileURLToPath(new URL("../../", import.meta.url));
const options = new Map<string, string>();
const flags = new Set<string>();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const arg = args[i];
  if (["--record", "--refresh"].includes(arg)) {
    flags.add(arg);
  } else if (
    ["--baseline", "--previous", "--output", "--input", "--reason"].includes(arg) &&
    args[i + 1]
  ) {
    options.set(arg, args[++i]);
  } else {
    throw new Error(`unknown/missing conservative-site argument: ${arg}`);
  }
}
const output = resolve(options.get("--output") ?? `${root}/target/conservative-sites`);
const input = resolve(options.get("--input") ?? `${output}/reports`);
const baselinePath = resolve(
  options.get("--baseline") ?? `${root}/bench/counts/conservative-sites.json`,
);
const previousPath = options.get("--previous");
const budget = existsSync(baselinePath)
  ? readSiteBudget(readFileSync(baselinePath, "utf8"))
  : undefined;
const previous =
  previousPath && existsSync(previousPath)
    ? readSiteBudget(readFileSync(previousPath, "utf8"))
    : undefined;
mkdirSync(output, { recursive: true });
if (!options.has("--input")) {
  // Never inherit a developer's corpus sampling or dev-profile selection.
  const env = { ...process.env };
  delete env.HD_CORPUS_DEV;
  env.HD_CORPUS_EVERY = "1";
  env.HD_CORPUS_PARTS = "bench";
  env.HD_CORPUS_OUT = input;
  env.HD_CACHE = `${output}/cache`;
  env.CARGO_BUILD_JOBS = "2";
  env.RUST_TEST_THREADS = "2";
  env.HD_TEST_JOBS = "2";
  try {
    const log = execFileSync(
      "cargo",
      [
        "test",
        "-q",
        "-j",
        "2",
        "-p",
        "hd_driver",
        "--test",
        "count_reasons",
        "count_reasons",
        "--",
        "--exact",
        "--ignored",
        "--nocapture",
      ],
      {
        cwd: root,
        env,
        encoding: "utf8",
        timeout: 600_000,
        maxBuffer: 64 * 1024 * 1024,
      },
    );
    writeFileSync(`${output}/build.log`, log);
  } catch (error) {
    const failed = error as { stdout?: string; stderr?: string };
    writeFileSync(`${output}/build.log`, `${failed.stdout ?? ""}\n${failed.stderr ?? ""}`);
    throw error;
  }
}
const programs = ["bench/study", "bench/runtime/progs"]
  .flatMap((dir) =>
    readdirSync(`${root}/${dir}`)
      .filter((name) => name.endsWith(".hd"))
      .map((name) => `${dir}/${name}`),
  )
  .sort();
const report = readSiteReport(
  readFileSync(`${input}/reasons.tsv`, "utf8"),
  readFileSync(`${input}/digests.tsv`, "utf8"),
  programs,
);
writeFileSync(`${output}/actual.json`, JSON.stringify(report, null, 2) + "\n");
const summary = siteTable(report, previous ?? budget);
writeFileSync(`${output}/summary.md`, summary);
process.stdout.write(summary);
if (flags.has("--record")) {
  writeFileSync(
    baselinePath,
    JSON.stringify(recordSites(report, budget, options.get("--reason")), null, 2) + "\n",
  );
} else {
  if (!budget) {
    throw new Error("missing conservative-site budget; seed it with --record");
  }
  const failures = compareSites(report, budget, previous);
  if (failures.length) {
    throw new Error(failures.join("\n"));
  }
  if (flags.has("--refresh")) {
    writeFileSync(baselinePath, JSON.stringify(recordSites(report, budget), null, 2) + "\n");
  }
}
