// Line coverage of the compiler core (#1442), per crate, with cargo-llvm-cov.
//
// The core is hd_syntax, hd_resolve, hd_types, hd_check, hd_tir, hd_mono,
// hd_opt and hd_wasm (owner, 2026-11-07). One instrumented release
// build runs, and its counts are merged into one report:
//   - each core crate's own tests (`core`, or one crate by its name);
//   - hd_driver's lib tests and stage tests (`driver`);
//   - the conformance suite in the three modes CI runs: default
//     (`conformance`), checked (`conformance-checked`) and checked optimized
//     (`conformance-checked-optimized`);
//   - std's runtime modules on the checked runtime (`std-rt-checked`);
//   - hd_command's CLI conformance cases (`cli-conformance`), the snapshot tests
//     that alone reach `hd fmt`, `hd doc` and `--tests`. They run the
//     instrumented `hd` on Node; the wasmtime engine, and with it the REPL,
//     is left out.
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
//   node scripts/coverage.ts --check --from REPORT --base PREVIOUS_REPORT
//   node scripts/coverage.ts --attribute                retain per-set evidence
//
// `--check` compares uncovered lines with the previous measured main run's
// artifact, mapping unchanged lines through git diff. New gaps and coverage
// lost on unchanged lines fail; deleted covered code cannot fail. Percentages
// are reported only. --base selects a downloaded report for an offline check.
//
// The full conformance suites run on CI only. Outside CI a conformance,
// std-rt-checked or cli-conformance set needs HD_CONFORMANCE_ONLY naming its
// cases (a std runtime file matches by path, as `ref_tests`); a narrow local
// run is, for example:
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
import { basename, dirname, join, relative, resolve } from "node:path";
import { parseArgs } from "node:util";
import { classifyRust, type FileLines, type LineKind } from "../bench/coverage/rust-lines.ts";
import {
  mapUncovered,
  newUncovered,
  previousMeasurement,
  type CoverageRun,
  type Range,
} from "../bench/coverage/history.ts";
import { contributions } from "../bench/coverage/contributions.ts";
import { archiveProfiles, restoreProfiles } from "../bench/coverage/profiles.ts";

const root = join(import.meta.dirname, "..");
const output = join(root, "bench", "coverage");
const work = join(root, "target", "coverage");
const coverageTarget = resolve(
  root,
  process.env.CARGO_LLVM_COV_TARGET_DIR ?? "target/llvm-cov-target",
);
const PRINT_LIMIT = 200;

const CORE = [
  "hd_syntax",
  "hd_resolve",
  "hd_types",
  "hd_check",
  "hd_tir",
  "hd_mono",
  "hd_opt",
  "hd_layout",
  "hd_wasm",
] as const;
type Crate = (typeof CORE)[number];

// The features CI's conformance builds turn on in the core: hd_wasm's
// checked report. wasmtime (the other engine) is left out; the default engine
// is Node.
const CRATE_FEATURES: Partial<Record<Crate, string>> = {
  hd_wasm: "hd_wasm/checked-report",
};
const DRIVER = ["-p", "hd_driver", "--features", "analysis,checked-report"];
const SUITE = "specification_conformance_passes_except_known_failures";
const STD_RT = "std_runtime_modules_pass_their_hd_tests_on_the_linear_backend";
const CLI_SUITE = "cli_conformance_passes_except_known_failures";
const MODE_KEYS = ["HD_CONFORMANCE_CHECKED", "HD_CONFORMANCE_PIPELINE", "HD_CONFORMANCE_ENGINE"];

type Run = {
  name: string;
  args: string[];
  env: Record<string, string>;
  // Narrowed by HD_CONFORMANCE_ONLY; undefined when the run isn't conformance.
  conformance?: "filtered";
};

const conformance = (name: string, test: string, env: Record<string, string>): Run => ({
  name,
  args: [...DRIVER, "--test", "conformance", "--", "--exact", test],
  env,
  conformance: "filtered",
});

// Measured >60s under instrumentation in run45186693397. Normal CI still
// runs these: repeated hash/order permutations, widths2..2048, and2k/20k test
// exports measure determinism/scale, not a new coverage scenario per repeat.
const COVERAGE_SKIPS = [
  "rows_determinism::row_polymorphic_programs_build_identically_in_every_order",
  "supplied_tuple_specialize::tuple_effect_summaries_borrow_small_and_shared_wide_operands",
  "test_plan::a_test_program_exports_two_entries_and_a_function_per_case",
];
const RUNS: Run[] = [
  {
    name: "driver",
    args: [
      ...DRIVER,
      "--test",
      "unit",
      "--test",
      "stages",
      "--test",
      "all",
      "--",
      ...COVERAGE_SKIPS.flatMap((name) => ["--skip", name]),
    ],
    env: {},
  },
  conformance("conformance", SUITE, {}),
  conformance("conformance-checked", SUITE, { HD_CONFORMANCE_CHECKED: "1" }),
  conformance("conformance-checked-optimized", SUITE, {
    HD_CONFORMANCE_CHECKED: "1",
    HD_CONFORMANCE_PIPELINE: "optimized",
  }),
  conformance("std-rt-checked", STD_RT, { HD_CONFORMANCE_CHECKED: "1" }),
  {
    name: "cli-conformance",
    args: ["-p", "hd_command", "--test", "cli_conformance", "--", "--exact", CLI_SUITE],
    env: {},
    conformance: "filtered",
  },
  // Trace names are lazy: exercise one traced dev link in its own CLI
  // process, without enabling tracing for the other coverage test sets.
  {
    name: "cli-trace",
    args: [
      "-p",
      "hd_command",
      "--test",
      "all",
      "--",
      "--exact",
      "work_report::a_trace_names_each_task_and_the_serial_phases",
    ],
    env: {},
  },
];
const SET_NAMES = ["core", ...CORE, ...RUNS.map((run) => run.name)];

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

// ---- options ----

const { values } = parseArgs({
  options: {
    check: { type: "boolean", default: false },
    attribute: { type: "boolean", default: false },
    base: { type: "string" },
    from: { type: "string" },
    only: { type: "string" },
    jobs: { type: "string" },
  },
});

function selectedSets(): string[] {
  if (values.check && values.from === undefined && process.env.HD_CONFORMANCE_ONLY) {
    fail("--check needs the whole suite; unset HD_CONFORMANCE_ONLY");
  }
  if (values.only === undefined) {
    // Run45437978053: checked adds no line beyond these retained sets.
    // Dropping it saves 424s + 2s reporting; dev must stay (4 shared lines).
    // Explicit --only still permits a diagnostic measurement of that mode.
    return SET_NAMES.filter(
      (name) => !(CORE as readonly string[]).includes(name) && name !== "conformance-checked",
    );
  }
  const sets = values.only.split(",").filter(Boolean);
  const unknown = sets.filter((set) => !SET_NAMES.includes(set));
  if (unknown.length > 0) {
    fail(`unknown set ${unknown.join(", ")}; the sets are ${SET_NAMES.join(", ")}`);
  }
  if (values.check) {
    fail("--check needs every run; leave out --only");
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
    if (run.conformance === "filtered" && !filtered) {
      fail(
        `${run.name} runs the whole suite, which runs on CI only; ` +
          "name cases with HD_CONFORMANCE_ONLY, or leave the set out with --only",
      );
    }
  }
}

// ---- measuring ----

function cargo(args: string[], env: Record<string, string> = {}): void {
  const childEnv: NodeJS.ProcessEnv = { ...process.env, CARGO_LLVM_COV_TARGET_DIR: coverageTarget };
  for (const key of MODE_KEYS) {
    delete childEnv[key];
  }
  Object.assign(childEnv, env);
  const shown = Object.entries(env).map(([key, value]) => `${key}=${value} `);
  console.log(`\n$ ${shown.join("")}cargo ${args.join(" ")}`);
  // cost: one invocation per selected test set/report; no unbounded retry.
  const result = spawnSync("cargo", args, {
    cwd: root,
    env: childEnv,
    stdio: "inherit",
    timeout: 75 * 60_000,
  });
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
  const setsDir = join(work, "sets");
  rmSync(setsDir, { recursive: true, force: true });
  const setCounts = new Map<string, Map<string, Map<number, number>>>();
  const profiles: [string, string][] = [];
  for (const run of runs) {
    const start = performance.now();
    cargo(["llvm-cov", "--no-report", "--release", ...jobs, ...run.args], {
      ...run.env,
      LLVM_PROFILE_FILE_NAME: `${run.name}-%p-%m.profraw`,
    });
    seconds[run.name] = Math.round((performance.now() - start) / 1000);
    if (!values.attribute) {
      continue;
    }
    const dir = join(setsDir, run.name);
    mkdirSync(dir, { recursive: true });
    const path = join(dir, "lcov.info");
    const reportStart = performance.now();
    cargo(["llvm-cov", "report", "--release", "--lcov", "--skip-functions", "--output-path", path]);
    seconds[`${run.name}-report`] = Math.round((performance.now() - reportStart) / 1000);
    setCounts.set(run.name, parseLcov(readFileSync(path, "utf8")));
    writeFileSync(
      join(dir, "timing.json"),
      JSON.stringify(
        {
          name: run.name,
          seconds: seconds[run.name],
          reportSeconds: seconds[`${run.name}-report`],
        },
        null,
        2,
      ) + "\n",
    );
    for (const profile of archiveProfiles(coverageTarget, dir)) {
      profiles.push(profile);
    }
  }
  // Restore the raw inputs for the ordinary merged report, without rerunning tests.
  restoreProfiles(profiles);
  if (values.attribute) {
    writeContributions(setCounts, seconds, setsDir);
  }
  mkdirSync(work, { recursive: true });
  const lcov = join(work, "lcov.info");
  const start = performance.now();
  cargo(["llvm-cov", "report", "--release", "--lcov", "--skip-functions", "--output-path", lcov]);
  seconds.report = Math.round((performance.now() - start) / 1000);
  const counts = parseLcov(readFileSync(lcov, "utf8"));
  return summarize(counts, {
    commit: process.env.GITHUB_SHA ?? git(["rev-parse", "HEAD"]).trim(),
    runs: runs.map((run) => run.name),
    conformanceOnly: process.env.HD_CONFORMANCE_ONLY || null,
    seconds,
  });
}

// Classify each current source file once across all sets. Only covered core
// non-test/non-invariant lines count as contributions, just like the main report.
function writeContributions(
  sets: Map<string, Map<string, Map<number, number>>>,
  seconds: Record<string, number>,
  dir: string,
): void {
  const classified = new Map<string, FileLines>();
  for (const files of sets.values()) {
    for (const absolute of files.keys()) {
      const file = relative(root, absolute).split("\\").join("/");
      if (crateOf(file) && !classified.has(file)) {
        classified.set(file, classifyRust(readFileSync(join(root, file), "utf8"), file));
      }
    }
  }
  const dirs = testFiles(classified);
  const covered = new Map<string, Set<string>>();
  for (const [name, files] of sets) {
    const lines = new Set<string>();
    for (const [absolute, counts] of files) {
      const file = relative(root, absolute).split("\\").join("/");
      const source = classified.get(file);
      if (!source || isTestFile(file, dirs)) {
        continue;
      }
      for (const [at, count] of counts) {
        if (count > 0 && source.kinds.get(at) === "code") {
          lines.add(`${file}:${at}`);
        }
      }
    }
    covered.set(name, lines);
  }
  const unique = contributions(covered);
  const rows = [...covered].map(([name, lines]) => ({
    name,
    seconds: seconds[name],
    reportSeconds: seconds[`${name}-report`],
    covered: lines.size,
    unique: unique.get(name),
  }));
  writeFileSync(join(dir, "contributions.json"), JSON.stringify(rows, null, 2) + "\n");
  console.log("Per-set measured core-line contributions:", rows);
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
    } else if (report.crates[crate]) {
      fail(`missing uncovered lines in stored measurement: ${list}`);
    }
  }
  return { report, uncovered };
}

function summaryTable(report: Report): string {
  const rows = CORE.flatMap((crate) => {
    const row = report.crates[crate];
    if (row === undefined) {
      return [];
    }
    return [
      `| ${crate} | ${row.covered} | ${row.total} | ${row.percent.toFixed(2)}% | ` +
        `${row.invariant.covered} / ${row.invariant.total} |`,
    ];
  });
  const times = Object.entries(report.seconds)
    .map(([name, seconds]) => `${name} ${seconds} s`)
    .join(", ");
  return [
    `### Core coverage at ${report.commit.slice(0, 9)}`,
    "",
    "| Crate | Covered | Lines | Percent | Invariant covered |",
    "| --- | ---: | ---: | ---: | ---: |",
    ...rows,
    "",
    `Runs: ${report.runs.join(", ")}. Time: ${times}.`,
    "",
  ].join("\n");
}

// ---- compare with the previous measured main run ----

function git(args: string[]): string {
  // cost: one diff of the two measured commits, or one commit-ID lookup.
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1 << 28,
    timeout: 30_000,
  });
}

function gh(args: string[]): string {
  // cost: one bounded run listing or the selected run's artifact download.
  return execFileSync("gh", args, { cwd: root, encoding: "utf8", timeout: 60_000 });
}

// Cost: at most 100 recent main runs, one ancestry/artifact lookup per
// candidate, and downloads only for live artifacts. No compiler rerun.
function previousReport(commit: string): Measured | undefined {
  if (values.base) {
    return readStored(values.base);
  }
  const repo =
    process.env.GITHUB_REPOSITORY ??
    gh(["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"]).trim();
  const data = JSON.parse(
    gh([
      "api",
      `repos/${repo}/actions/workflows/coverage.yml/runs?branch=main&per_page=100`,
      "--jq",
      "{workflow_runs: [.workflow_runs[] | {id, head_sha}]}",
    ]),
  ) as { workflow_runs: CoverageRun[] };
  const previous = previousMeasurement(
    data.workflow_runs,
    Number(process.env.GITHUB_RUN_ID ?? Infinity),
    (sha) => {
      // cost: at most 100 ancestry queries over the recent runs.
      const ancestor = spawnSync("git", ["merge-base", "--is-ancestor", sha, commit], {
        cwd: root,
        timeout: 10_000,
      });
      if (ancestor.status === 1) {
        return false;
      }
      if (ancestor.status !== 0) {
        throw new Error(`cannot map previous coverage commit ${sha}`);
      }
      return true;
    },
    (run) => {
      const artifacts = JSON.parse(
        gh(["api", `repos/${repo}/actions/runs/${run.id}/artifacts?name=core-coverage`]),
      ) as { artifacts: { name: string; expired: boolean }[] };
      if (!artifacts.artifacts.some((item) => item.name === "core-coverage" && !item.expired)) {
        return undefined;
      }
      const dir = join(work, "previous", String(run.id));
      rmSync(dir, { recursive: true, force: true });
      mkdirSync(dir, { recursive: true });
      gh([
        "run",
        "download",
        String(run.id),
        "--repo",
        repo,
        "--name",
        "core-coverage",
        "--dir",
        dir,
      ]);
      const path = join(dir, "bench", "coverage", "report.json");
      // always() also uploads per-set timings after a timeout. Those are
      // useful diagnostics, but are not a completed coverage measurement.
      if (!existsSync(path)) {
        return undefined;
      }
      const measured = readStored(path);
      if (measured.report.commit !== run.head_sha) {
        throw new Error("coverage artifact commit differs from its run");
      }
      if (measured.report.conformanceOnly || CORE.some((crate) => !measured.report.crates[crate])) {
        return undefined;
      }
      return measured;
    },
  );
  if (!previous) {
    console.log("No previous complete main measurement in the recent run history: report only.");
  }
  return previous;
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

function check({ report, uncovered }: Measured, previous: Measured | undefined): boolean {
  if (!previous) {
    return true;
  }
  if (report.conformanceOnly || previous.report.conformanceOnly) {
    fail("a comparison needs complete measurements, not named conformance subsets");
  }
  const before = previous.report.commit;
  console.log(`Comparing uncovered lines with main's measurement at ${before.slice(0, 9)}.`);
  const diff = git([
    "-c",
    "core.quotePath=false",
    "diff",
    "-U0",
    "--find-renames",
    "--no-color",
    "--no-ext-diff",
    before,
    report.commit,
    "--",
    ...CORE.map((crate) => `crates/${crate}/src`),
  ]);
  const mapped = mapUncovered([...previous.uncovered.values()].flat(), diff);
  const mappedCrates = new Map<Crate, Range[]>();
  for (const range of mapped) {
    const crate = crateOf(range.file);
    if (crate) {
      const ranges = mappedCrates.get(crate) ?? [];
      ranges.push(range);
      mappedCrates.set(crate, ranges);
    }
  }
  let ok = true;
  for (const crate of CORE) {
    if (!report.crates[crate]) {
      console.log(`${crate}: not measured`);
      ok = false;
      continue;
    }
    const fresh = newUncovered(uncovered.get(crate) ?? [], mappedCrates.get(crate) ?? []);
    if (fresh.length) {
      ok = false;
      printRanges(`${crate}: new or newly uncovered ranges`, fresh);
    }
  }
  return ok;
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
  const table = summaryTable(measured.report);
  console.log(`\n${table}`);
  if (values.from === undefined) {
    writeFileSync(join(output, "summary.md"), table);
  }
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, table);
  }
  if (values.check && !check(measured, previousReport(measured.report.commit))) {
    process.exitCode = 1;
  }
}

try {
  main();
} catch (error) {
  fail(String(error));
}
