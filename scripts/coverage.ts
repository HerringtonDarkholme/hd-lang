// Line coverage of the compiler core (#1442), per crate, with cargo-llvm-cov.
//
// The core is hd_syntax, hd_resolve, hd_types, hd_check, hd_tir, hd_mono,
// hd_opt and hd_wasm_linear (owner, 2026-11-07). One instrumented release
// build runs, and its counts are merged into one report:
//   - each core crate's own tests (`core`, or one crate by its name);
//   - hd_driver's lib tests and stage tests (`driver`);
//   - the conformance suite in the three modes CI runs: default
//     (`conformance`), checked (`conformance-checked`) and checked optimized
//     (`conformance-checked-optimized`);
//   - std's runtime modules on the checked runtime (`std-rt-checked`).
//
// Only lines under crates/<core>/src count. bench/coverage/rust-lines.ts lexes
// each file: blank and comment lines and `#[cfg(test)]` code leave the report,
// and lines that run only when an invariant breaks (`unreachable!`, `panic!`,
// `debug_assert!`, verifier findings) are counted apart as "invariant". A
// crate's percent is its covered non-invariant lines over all of them.
//
// Writes bench/coverage/report.json (per crate: covered, total, percent, the
// invariant lines, and the time of each run), bench/coverage/summary.md and
// bench/coverage/uncovered/<crate>.txt and <crate>.invariant.txt (one
// `path:first-last` range per line). The raw LCOV is target/coverage/lcov.info.
//
// Usage:
//   node scripts/coverage.ts [--only SETS] [--jobs N]   measure and report
//   node scripts/coverage.ts --check                    also fail on a drop
//   node scripts/coverage.ts --record [--from REPORT]   rewrite the baseline
//   node scripts/coverage.ts --check --from REPORT      check a stored report
//
// `--check` fails when a crate's percent is more than 0.05 points under
// bench/coverage/baseline.json, and prints its uncovered ranges on lines
// changed since the baseline's commit. With no baseline yet it only reports.
// `--record` rewrites the baseline from the measured report, or from a
// report.json that CI uploaded (its uncovered/ folder beside it). Both need
// every run. The baseline only moves by `--record`: CI measures on a schedule,
// and by the time a run ends main has usually moved on, so the counts gate's
// automatic refresh (which yields to a newer main) would rarely apply.
//
// The full conformance suite runs on CI only. Outside CI a conformance set
// needs HD_CONFORMANCE_ONLY naming its cases, and `std-rt-checked` (which
// HD_CONFORMANCE_ONLY doesn't narrow) is refused; a narrow local run is, for
// example:
//   HD_CONFORMANCE_ONLY=a,b node scripts/coverage.ts --only hd_opt,conformance --jobs 2
// On macOS, set LLVM_COV and LLVM_PROFDATA to `xcrun --find llvm-cov` and
// `xcrun --find llvm-profdata`: Homebrew's LLVM can't read rustc's profiles.

import { spawnSync, execFileSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative } from "node:path";
import { parseArgs } from "node:util";
import { classifyRust, type FileLines, type LineKind } from "../bench/coverage/rust-lines.ts";

const root = join(import.meta.dirname, "..");
const output = join(root, "bench", "coverage");
const work = join(root, "target", "coverage");
const baselinePath = join(output, "baseline.json");
const TOLERANCE = 0.05;
const PRINT_LIMIT = 200;

const CORE = [
  "hd_syntax",
  "hd_resolve",
  "hd_types",
  "hd_check",
  "hd_tir",
  "hd_mono",
  "hd_opt",
  "hd_wasm_linear",
] as const;
type Crate = (typeof CORE)[number];

// The features CI's conformance builds turn on in the core: hd_check's
// analysis walk and hd_wasm_linear's checked report. wasmtime (the other
// engine) is left out; the default engine is Node.
const CRATE_FEATURES: Partial<Record<Crate, string>> = {
  hd_check: "hd_check/analysis",
  hd_wasm_linear: "hd_wasm_linear/checked-report",
};
const DRIVER = ["-p", "hd_driver", "--features", "analysis,checked-report"];
const SUITE = "specification_conformance_passes_except_known_failures";
const STD_RT = "std_runtime_modules_pass_their_hd_tests_on_the_linear_backend";
const MODE_KEYS = ["HD_CONFORMANCE_CHECKED", "HD_CONFORMANCE_PIPELINE", "HD_CONFORMANCE_ENGINE"];

type Run = {
  name: string;
  args: string[];
  env: Record<string, string>;
  // Narrowed by HD_CONFORMANCE_ONLY; undefined when the run isn't conformance.
  conformance?: "filtered" | "whole";
};

const conformance = (name: string, test: string, env: Record<string, string>): Run => ({
  name,
  args: [...DRIVER, "--test", "conformance", "--", "--exact", test],
  env,
  conformance: test === SUITE ? "filtered" : "whole",
});

const RUNS: Run[] = [
  { name: "driver", args: [...DRIVER, "--lib", "--test", "stages", "--test", "all"], env: {} },
  conformance("conformance", SUITE, {}),
  conformance("conformance-checked", SUITE, { HD_CONFORMANCE_CHECKED: "1" }),
  conformance("conformance-checked-optimized", SUITE, {
    HD_CONFORMANCE_CHECKED: "1",
    HD_CONFORMANCE_PIPELINE: "optimized",
  }),
  conformance("std-rt-checked", STD_RT, { HD_CONFORMANCE_CHECKED: "1" }),
];
const SET_NAMES = ["core", ...CORE, ...RUNS.map((run) => run.name)];

type Range = { file: string; first: number; last: number };
type Lines = { covered: number; total: number };
type CrateReport = Lines & { percent: number; invariant: Lines };
type Report = {
  commit: string;
  runs: string[];
  // HD_CONFORMANCE_ONLY of a narrowed run, which can't make a baseline.
  conformanceOnly: string | null;
  seconds: Record<string, number>;
  crates: Partial<Record<Crate, CrateReport>>;
};
type Measured = { report: Report; uncovered: Map<Crate, Range[]> };
type Baseline = { commit: string; crates: Partial<Record<Crate, number>> };

// ---- options ----

const { values } = parseArgs({
  options: {
    check: { type: "boolean", default: false },
    record: { type: "boolean", default: false },
    from: { type: "string" },
    only: { type: "string" },
    jobs: { type: "string" },
  },
});

function selectedSets(): string[] {
  if (
    (values.check || values.record) &&
    values.from === undefined &&
    process.env.HD_CONFORMANCE_ONLY
  ) {
    fail("--check and --record need the whole suite; unset HD_CONFORMANCE_ONLY");
  }
  if (values.only === undefined) {
    return SET_NAMES.filter((name) => !(CORE as readonly string[]).includes(name));
  }
  const sets = values.only.split(",").filter(Boolean);
  const unknown = sets.filter((set) => !SET_NAMES.includes(set));
  if (unknown.length > 0) {
    fail(`unknown set ${unknown.join(", ")}; the sets are ${SET_NAMES.join(", ")}`);
  }
  if (values.check || values.record) {
    fail("--check and --record need every run; leave out --only");
  }
  return sets;
}

function fail(message: string): never {
  process.stderr.write(`coverage: ${message}\n`);
  process.exit(2);
}

// The full suite runs on CI only (owner): outside CI, conformance names its cases.
function guardLocalRuns(runs: Run[]): void {
  const allowed =
    process.env.CI === "true" ||
    process.env.FULL_SUITE_ONLY_AS_LAST_STEP_OF_A_TRULY_CROSS_CUTTING_CHANGE === "1";
  if (allowed) {
    return;
  }
  const filtered = Boolean(process.env.HD_CONFORMANCE_ONLY);
  for (const run of runs) {
    if (run.conformance === "whole" || (run.conformance === "filtered" && !filtered)) {
      fail(
        `${run.name} runs the whole suite, which runs on CI only; ` +
          "name cases with HD_CONFORMANCE_ONLY, or leave the set out with --only",
      );
    }
  }
}

// ---- measuring ----

function cargo(args: string[], env: Record<string, string> = {}): void {
  const childEnv: NodeJS.ProcessEnv = { ...process.env };
  for (const key of MODE_KEYS) {
    delete childEnv[key];
  }
  Object.assign(childEnv, env);
  const shown = Object.entries(env).map(([key, value]) => `${key}=${value} `);
  console.log(`\n$ ${shown.join("")}cargo ${args.join(" ")}`);
  const result = spawnSync("cargo", args, { cwd: root, env: childEnv, stdio: "inherit" });
  if (result.status !== 0) {
    fail(`cargo ${args.slice(0, 2).join(" ")} failed (${result.status ?? result.signal})`);
  }
}

function plannedRuns(sets: string[]): Run[] {
  const crates = sets.includes("core") ? [...CORE] : CORE.filter((name) => sets.includes(name));
  const runs: Run[] = [];
  if (crates.length > 0) {
    const features = crates.flatMap((name) => CRATE_FEATURES[name] ?? []);
    const featureArgs = features.length > 0 ? ["--features", features.join(",")] : [];
    runs.push({
      name: crates.length === CORE.length ? "core" : crates.join("+"),
      args: [...crates.flatMap((name) => ["-p", name]), ...featureArgs],
      env: {},
    });
  }
  return [...runs, ...RUNS.filter((run) => sets.includes(run.name))];
}

function measure(sets: string[]): Measured {
  const runs = plannedRuns(sets);
  guardLocalRuns(runs);
  const jobs = values.jobs === undefined ? [] : ["-j", values.jobs];
  const seconds: Record<string, number> = {};
  // Stale test binaries of the workspace would map old line numbers, so each
  // measurement rebuilds the workspace crates (dependencies stay built).
  cargo(["llvm-cov", "clean", "--workspace"]);
  for (const run of runs) {
    const start = performance.now();
    cargo(["llvm-cov", "--no-report", "--release", ...jobs, ...run.args], run.env);
    seconds[run.name] = Math.round((performance.now() - start) / 1000);
  }
  mkdirSync(work, { recursive: true });
  const lcov = join(work, "lcov.info");
  const start = performance.now();
  cargo(["llvm-cov", "report", "--release", "--lcov", "--output-path", lcov]);
  seconds.report = Math.round((performance.now() - start) / 1000);
  const counts = parseLcov(readFileSync(lcov, "utf8"));
  return summarize(counts, {
    commit: process.env.GITHUB_SHA ?? git(["rev-parse", "HEAD"]).trim(),
    runs: runs.map((run) => run.name),
    conformanceOnly: process.env.HD_CONFORMANCE_ONLY || null,
    seconds,
  });
}

// Absolute source path to its line counts.
function parseLcov(text: string): Map<string, Map<number, number>> {
  const files = new Map<string, Map<number, number>>();
  let current = new Map<number, number>();
  for (const line of text.split("\n")) {
    if (line.startsWith("SF:")) {
      const file = line.slice(3);
      current = files.get(file) ?? new Map();
      files.set(file, current);
    } else if (line.startsWith("DA:")) {
      const [at, count] = line.slice(3).split(",");
      const lineNumber = Number(at);
      current.set(lineNumber, Math.max(current.get(lineNumber) ?? 0, Number(count)));
    }
  }
  return files;
}

// ---- classifying ----

function crateOf(file: string): Crate | undefined {
  const parts = file.split("/");
  const crate = CORE.find((name) => name === parts[1]);
  return parts[0] === "crates" && parts[2] === "src" && file.endsWith(".rs") ? crate : undefined;
}

// The module folder of a file's `mod name;` children: src/a.rs -> src/a/.
function moduleDir(file: string): string {
  const name = basename(file);
  const folder = dirname(file);
  return ["lib.rs", "main.rs", "mod.rs"].includes(name)
    ? folder
    : join(folder, name.slice(0, -".rs".length));
}

// Files that are test code: declared by `#[cfg(test)] mod name;`, or under one.
function testFiles(classified: Map<string, FileLines>): string[] {
  const dirs: string[] = [];
  for (const [file, lines] of classified) {
    if (lines.wholeFileTest) {
      dirs.push(file.slice(0, -".rs".length));
    }
    for (const name of lines.testModules) {
      dirs.push(join(moduleDir(file), name));
    }
  }
  return dirs;
}

function isTestFile(file: string, dirs: string[]): boolean {
  const stem = file.endsWith("/mod.rs") ? dirname(file) : file.slice(0, -".rs".length);
  return dirs.some((dir) => stem === dir || stem.startsWith(`${dir}/`));
}

type Tally = {
  code: Lines;
  invariant: Lines;
  uncovered: Range[];
  invariantUncovered: Range[];
};

function newTally(): Tally {
  return {
    code: { covered: 0, total: 0 },
    invariant: { covered: 0, total: 0 },
    uncovered: [],
    invariantUncovered: [],
  };
}

// Adds `line` to `ranges`: a range grows while its file's counted lines stay uncovered.
function extend(ranges: Range[], file: string, line: number, open: boolean): void {
  const last = ranges.at(-1);
  if (open && last?.file === file) {
    last.last = line;
  } else {
    ranges.push({ file, first: line, last: line });
  }
}

function tallyFile(
  tally: Tally,
  file: string,
  counts: Map<number, number>,
  lines: FileLines,
): void {
  // The previous counted line's kind when it was uncovered: a range grows
  // only over uncovered lines of its own kind, with no other counted line between.
  let open: LineKind | undefined;
  for (const [line, count] of [...counts].sort(([left], [right]) => left - right)) {
    const kind = lines.kinds.get(line);
    if (kind !== "code" && kind !== "invariant") {
      continue;
    }
    const lineCounts = kind === "code" ? tally.code : tally.invariant;
    lineCounts.total++;
    lineCounts.covered += count > 0 ? 1 : 0;
    if (count === 0) {
      const ranges = kind === "code" ? tally.uncovered : tally.invariantUncovered;
      extend(ranges, file, line, open === kind);
    }
    open = count === 0 ? kind : undefined;
  }
}

const percentOf = ({ covered, total }: Lines): number =>
  total === 0 ? 100 : Math.floor((covered / total) * 10000) / 100;

function summarize(
  counts: Map<string, Map<number, number>>,
  meta: Omit<Report, "crates">,
): Measured {
  const byCrate = new Map<Crate, Map<string, Map<number, number>>>();
  for (const [absolute, lines] of counts) {
    const file = relative(root, absolute).split("\\").join("/");
    const crate = crateOf(file);
    if (crate !== undefined) {
      const files = byCrate.get(crate) ?? new Map();
      files.set(file, lines);
      byCrate.set(crate, files);
    }
  }
  const report: Report = { ...meta, crates: {} };
  const uncovered = new Map<Crate, Range[]>();
  for (const crate of CORE) {
    const files = byCrate.get(crate);
    if (files === undefined) {
      continue;
    }
    const tally = tallyCrate(files);
    report.crates[crate] = {
      ...tally.code,
      percent: percentOf(tally.code),
      invariant: tally.invariant,
    };
    uncovered.set(crate, tally.uncovered);
    writeRanges(crate, tally.uncovered, tally.invariantUncovered);
  }
  return { report, uncovered };
}

function tallyCrate(files: Map<string, Map<number, number>>): Tally {
  const sorted = [...files].sort(([left], [right]) => left.localeCompare(right));
  const classified = new Map<string, FileLines>();
  for (const [file] of sorted) {
    classified.set(file, classifyRust(readFileSync(join(root, file), "utf8"), file));
  }
  const dirs = testFiles(classified);
  const tally = newTally();
  for (const [file, counts] of sorted) {
    const lines = classified.get(file);
    if (lines !== undefined && !isTestFile(file, dirs)) {
      tallyFile(tally, file, counts, lines);
    }
  }
  return tally;
}

// ---- outputs ----

const formatRange = ({ file, first, last }: Range): string =>
  first === last ? `${file}:${first}` : `${file}:${first}-${last}`;

function parseRange(text: string): Range {
  const [file, span] = [
    text.slice(0, text.lastIndexOf(":")),
    text.slice(text.lastIndexOf(":") + 1),
  ];
  const [first, last = first] = span.split("-");
  return { file, first: Number(first), last: Number(last) };
}

function writeRanges(crate: Crate, code: Range[], invariant: Range[]): void {
  const dir = join(output, "uncovered");
  mkdirSync(dir, { recursive: true });
  const body = (ranges: Range[]): string =>
    ranges.map((range) => `${formatRange(range)}\n`).join("");
  writeFileSync(join(dir, `${crate}.txt`), body(code));
  writeFileSync(join(dir, `${crate}.invariant.txt`), body(invariant));
}

function readStored(path: string): Measured {
  const report = JSON.parse(readFileSync(path, "utf8")) as Report;
  const uncovered = new Map<Crate, Range[]>();
  for (const crate of CORE) {
    const list = join(dirname(path), "uncovered", `${crate}.txt`);
    if (existsSync(list)) {
      const lines = readFileSync(list, "utf8").split("\n").filter(Boolean);
      uncovered.set(crate, lines.map(parseRange));
    }
  }
  return { report, uncovered };
}

function readBaseline(): Baseline | undefined {
  return existsSync(baselinePath)
    ? (JSON.parse(readFileSync(baselinePath, "utf8")) as Baseline)
    : undefined;
}

function summaryTable(report: Report, baseline: Baseline | undefined): string {
  const rows = CORE.flatMap((crate) => {
    const row = report.crates[crate];
    if (row === undefined) {
      return [];
    }
    const base = baseline?.crates[crate];
    const delta = base === undefined ? "" : ` (${(row.percent - base).toFixed(2)})`;
    return [
      `| ${crate} | ${row.covered} | ${row.total} | ${row.percent.toFixed(2)}% | ` +
        `${base === undefined ? "-" : `${base.toFixed(2)}%`}${delta} | ` +
        `${row.invariant.covered} / ${row.invariant.total} |`,
    ];
  });
  const times = Object.entries(report.seconds)
    .map(([name, seconds]) => `${name} ${seconds} s`)
    .join(", ");
  return [
    `### Core coverage at ${report.commit.slice(0, 9)}`,
    "",
    "| Crate | Covered | Lines | Percent | Baseline | Invariant covered |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
    ...rows,
    "",
    `Runs: ${report.runs.join(", ")}. Time: ${times}.`,
    "",
  ].join("\n");
}

// ---- check and record ----

function git(args: string[]): string {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
}

// Lines added or changed between two commits, per file; undefined when git can't tell.
function changedLines(
  from: string,
  to: string,
  crate: Crate,
): Map<string, Set<number>> | undefined {
  let diff: string;
  try {
    diff = git([
      "diff",
      "-U0",
      "--no-color",
      "--no-ext-diff",
      from,
      to,
      "--",
      `crates/${crate}/src`,
    ]);
  } catch {
    return undefined;
  }
  const changed = new Map<string, Set<number>>();
  let lines = new Set<number>();
  for (const line of diff.split("\n")) {
    const file = /^\+\+\+ b\/(.*)$/.exec(line);
    const hunk = /^@@ -\S+ \+(\d+)(?:,(\d+))? @@/.exec(line);
    if (file) {
      lines = new Set();
      changed.set(file[1], lines);
    } else if (hunk) {
      const first = Number(hunk[1]);
      const count = hunk[2] === undefined ? 1 : Number(hunk[2]);
      for (let at = first; at < first + count; at++) {
        lines.add(at);
      }
    }
  }
  return changed;
}

function newRanges(ranges: Range[], changed: Map<string, Set<number>>): Range[] {
  return ranges.filter((range) => {
    const lines = changed.get(range.file);
    for (let at = range.first; lines && at <= range.last; at++) {
      if (lines.has(at)) {
        return true;
      }
    }
    return false;
  });
}

function printRanges(title: string, ranges: Range[]): void {
  console.log(`  ${title} (${ranges.length}):`);
  for (const range of ranges.slice(0, PRINT_LIMIT)) {
    console.log(`    ${formatRange(range)}`);
  }
  if (ranges.length > PRINT_LIMIT) {
    console.log(`    ... ${ranges.length - PRINT_LIMIT} more in the uncovered/ artifact`);
  }
}

function check({ report, uncovered }: Measured, baseline: Baseline | undefined): boolean {
  if (baseline === undefined) {
    console.log("No bench/coverage/baseline.json yet; record one with --record.");
    return true;
  }
  let ok = true;
  for (const crate of CORE) {
    const row = report.crates[crate];
    const base = baseline.crates[crate];
    if (row === undefined || base === undefined) {
      console.log(`${crate}: ${row === undefined ? "not measured" : "no baseline"}`);
      ok &&= row !== undefined;
      continue;
    }
    if (row.percent > base + TOLERANCE) {
      console.log(`${crate}: ${row.percent}% is above the baseline ${base}%; --record raises it`);
    }
    if (row.percent >= base - TOLERANCE) {
      continue;
    }
    ok = false;
    console.log(`${crate}: ${row.percent}% fell below the baseline ${base}%`);
    const ranges = uncovered.get(crate) ?? [];
    const changed = changedLines(baseline.commit, report.commit, crate);
    if (changed === undefined) {
      printRanges(`uncovered ranges (no diff from ${baseline.commit.slice(0, 9)})`, ranges);
    } else {
      printRanges(
        `uncovered ranges on lines changed since ${baseline.commit.slice(0, 9)}`,
        newRanges(ranges, changed),
      );
    }
  }
  return ok;
}

function record(report: Report): void {
  const missing = CORE.filter((crate) => report.crates[crate] === undefined);
  const runs = ["core", ...RUNS.map((run) => run.name)];
  const partial = runs.some((run) => !report.runs.includes(run)) || report.conformanceOnly;
  if (missing.length > 0 || partial) {
    fail(`a baseline needs every crate and run; this report has ${report.runs.join(", ")}`);
  }
  const crates: Partial<Record<Crate, number>> = {};
  for (const crate of CORE) {
    crates[crate] = report.crates[crate]?.percent;
  }
  const baseline: Baseline = { commit: report.commit, crates };
  writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
  console.log(`Recorded ${relative(root, baselinePath)} at ${report.commit.slice(0, 9)}.`);
}

function main(): void {
  const sets = selectedSets();
  let measured: Measured;
  if (values.from === undefined) {
    rmSync(join(output, "uncovered"), { recursive: true, force: true });
    measured = measure(sets);
    writeFileSync(join(output, "report.json"), `${JSON.stringify(measured.report, null, 2)}\n`);
  } else {
    measured = readStored(values.from);
  }
  const baseline = readBaseline();
  const table = summaryTable(measured.report, baseline);
  console.log(`\n${table}`);
  if (values.from === undefined) {
    writeFileSync(join(output, "summary.md"), table);
  }
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, table);
  }
  if (values.record) {
    record(measured.report);
  } else if (values.check && !check(measured, baseline)) {
    process.exitCode = 1;
  }
}

main();
