// The metric runner: runs the selected metric scripts against one `hd` and
// judges each target. See README.md.
//
// Usage: node --experimental-strip-types bench/metrics/run.ts
//          [--hd "COMMAND"] [--only NAME[,NAME...]] [--pillar N] [--list] [--keep-temp]
//          [--suite] [--max] [--long] [--json FILE]
//
// It prints one line per target as it is judged, then a summary table, and
// exits 1 when any target fails. An n/a target never fails the run.

import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { parseArgs } from "node:util";

import { killLiveRuns, parseHdCommand } from "./lib/hd.ts";
import type { Metric, TargetResult } from "./lib/metric.ts";
import { keepTempDirs, removeAllTempDirs } from "./lib/tmp.ts";
import { METRICS } from "./scripts/index.ts";

const REPO_ROOT = join(import.meta.dirname, "..", "..");
const DEFAULT_HD = "target/release/hd";

const STATUS_LABEL = { pass: "PASS", fail: "FAIL", "n/a": "N/A " } as const;

function usage(): string {
  return [
    "usage: run.ts [--hd COMMAND] [--only NAME[,NAME...]] [--pillar N] [--list] [--keep-temp]",
    "              [--suite] [--max] [--long] [--json FILE]",
    "",
    `--hd COMMAND   the hd under test (default: ${DEFAULT_HD}, from the repository root)`,
    "--only NAMES   run only these metrics; repeatable or comma-separated",
    "--pillar N     run only the metrics of pillar N (1, 2, 3, or gate)",
    "--list         list the metrics and exit",
    "--keep-temp    keep temporary directories, for debugging",
    "--suite        also run the metrics that take minutes (the conformance pass rate)",
    "--max          also run the largest scales (64 concurrent checks)",
    "--long         run long-run-memory for 10 minutes instead of 60 s",
    "--json FILE    write the full run as JSON, including failed and n/a targets",
  ].join("\n");
}

/** Selects metrics by name and pillar; unknown names throw. */
export function selectMetrics(
  all: readonly Metric[],
  only: readonly string[],
  pillar: string | undefined,
): Metric[] {
  const names = only
    .flatMap((value) => value.split(","))
    .map((value) => value.trim())
    .filter(Boolean);
  const unknown = names.filter((name) => !all.some((metric) => metric.name === name));
  if (unknown.length) throw new Error(`unknown metric: ${unknown.join(", ")}`);
  return all.filter(
    (metric) =>
      (names.length === 0 || names.includes(metric.name)) &&
      (pillar === undefined || String(metric.pillar) === pillar),
  );
}

/** A plain-text table with a header row and aligned columns. */
export function formatTable(rows: readonly (readonly string[])[]): string {
  const widths = rows[0]!.map((_, column) => Math.max(...rows.map((row) => row[column]!.length)));
  const line = (row: readonly string[]) =>
    row
      .map((cell, column) => cell.padEnd(widths[column]!))
      .join(" | ")
      .trimEnd();
  return [
    line(rows[0]!),
    widths.map((width) => "-".repeat(width)).join("-|-"),
    ...rows.slice(1).map(line),
  ].join("\n");
}

const resultLine = (result: TargetResult): string =>
  `${STATUS_LABEL[result.status]} ${result.metric}: ${result.name}: ${result.value} (target ${result.target})${
    result.note ? ` — ${result.note}` : ""
  }`;

export interface MetricsRun {
  readonly hd: string;
  readonly commit: string;
  readonly finished: string;
  readonly results: readonly TargetResult[];
  readonly machine: { readonly runner: string; readonly cpus: number; readonly cpu: string };
}

const targetKey = (result: Pick<TargetResult, "metric" | "name">): string =>
  JSON.stringify([result.metric, result.name]);
const tsvCell = (value: string): string => value.replace(/[\t\r\n]/g, " ");
const markdownCell = (value: string): string =>
  value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll("|", "&#124;")
    .replace(/[\r\n]/g, " ");
const counts = (run: MetricsRun): number[] =>
  ["pass", "fail", "n/a"].map((status) => run.results.filter((r) => r.status === status).length);
const historyRows = (text: string): string[][] =>
  text
    .trimEnd()
    .split("\n")
    .filter(Boolean)
    .map((line) => line.split("\t"));

/** Keep earlier rows and expand the schema when targets are added or removed. */
export function appendMetricsHistory(history: string, run: MetricsRun): string {
  const [oldHeader = [], ...oldRows] = historyRows(history);
  const values = new Map<string, string>([
    ["date", run.finished.slice(0, 10)],
    ["commit", run.commit],
    ...counts(run).map(
      (value, i) => [["pass", "fail", "n/a"][i]!, String(value)] as [string, string],
    ),
    ["finished", run.finished],
    ["hd", run.hd],
    ["runner", run.machine.runner],
    ["cpus", String(run.machine.cpus)],
    ["cpu", run.machine.cpu],
  ]);
  for (const result of run.results) {
    values.set("value " + targetKey(result), result.value);
    values.set("status " + targetKey(result), result.status);
  }
  const header = [...new Set([...oldHeader, ...values.keys()])];
  const rows = oldRows
    .filter((row) => row[oldHeader.indexOf("finished")] !== run.finished)
    .map((row) => header.map((key) => row[oldHeader.indexOf(key)] ?? ""));
  rows.push(header.map((key) => values.get(key) ?? ""));
  return [header, ...rows].map((row) => row.map(tsvCell).join("\t")).join("\n") + "\n";
}

/** Recover the previous run, including an earlier run on the same day. */
function previousMetrics(history: string, run: MetricsRun): readonly TargetResult[] | undefined {
  const [header = [], ...rows] = historyRows(history);
  const row = rows.findLast((cells) => cells[header.indexOf("finished")] !== run.finished);
  if (!row) return undefined;
  return header.flatMap((key, i) => {
    if (!key.startsWith("value ") || !row[i]) return [];
    const [metric, name] = JSON.parse(key.slice(6)) as [string, string];
    const status = row[header.indexOf("status " + key.slice(6))];
    if (status !== "pass" && status !== "fail" && status !== "n/a") return [];
    return [{ metric, name, value: row[i]!, status, target: "" }];
  });
}

// Compare like quantities, including throughput scale changes and composite
// CPU/RSS values. Non-numeric measurements still report status changes.
function quantities(value: string): { amount: number; unit: string; timing: boolean }[] | null {
  const units: Record<string, [number, string]> = {
    s: [1000, "ms"],
    ms: [1, "ms"],
    µs: [0.001, "ms"],
    ns: [0.000001, "ms"],
    MB: [1048576, "B"],
    "M/s": [1000000, "/s"],
    "k/s": [1000, "/s"],
  };
  const found = value.split(", ").map((part) => {
    const match =
      /^(-?(?:\d+(?:\.\d*)?|\.\d+))\s*(ms|s|ns|µs|MB\/s|M\/s|k\/s|\/s|MB|B|tokens|%|x|B\/iter)?$/.exec(
        part,
      );
    if (!match) return null;
    const unit = match[2] ?? "count";
    const [scale, normalized] = units[unit] ?? [1, unit];
    return { amount: Number(match[1]) * scale, unit: normalized, timing: normalized === "ms" };
  });
  return found.every((part) => part !== null) ? found : null;
}

function movement(before: string, after: string): { large: boolean; noise: boolean } {
  const left = quantities(before),
    right = quantities(after);
  if (
    !left ||
    !right ||
    left.length !== right.length ||
    left.some((part, i) => part.unit !== right[i]!.unit)
  )
    return { large: false, noise: false };
  const exceeds = left.some(
    (part, i) =>
      Math.abs(part.amount - right[i]!.amount) > Math.abs(part.amount) * 0.1 + Number.EPSILON,
  );
  return {
    large: exceeds,
    noise: !exceeds && before !== after && left.some((part) => part.timing),
  };
}

/** The same comparison section is published in README and the Actions summary. */
export function metricsChanges(run: MetricsRun, previous?: readonly TargetResult[]): string {
  const lines = ["## Changed since the previous run", ""];
  if (!previous) return lines.concat("First recorded run.", "").join("\n");
  const old = new Map(previous.map((result) => [targetKey(result), result]));
  for (const result of run.results) {
    const before = old.get(targetKey(result));
    if (before && (before.status !== result.status || movement(before.value, result.value).large))
      lines.push(
        `- ${markdownCell(result.metric + ": " + result.name)}: ${markdownCell(before.status + " " + before.value)} → ${markdownCell(result.status + " " + result.value)}`,
      );
  }
  if (lines.length === 2) lines.push("No status flips or value movements greater than 10%.");
  return lines.concat("").join("\n");
}

export function metricsReadme(
  run: MetricsRun,
  previous: readonly TargetResult[] | undefined,
  days: readonly string[],
): string {
  const old = new Map(previous?.map((result) => [targetKey(result), result]));
  const lines = [
    "# Daily goal metrics",
    "",
    `Finished: ${run.finished}. Commit: ${run.commit}.`,
    `Machine: ${markdownCell(run.machine.runner)}, ${run.machine.cpus} CPUs (${markdownCell(run.machine.cpu)}).`,
    `Compiler: ${markdownCell(run.hd)}.`,
    "",
    `${counts(run)[0]} passed, ${counts(run)[1]} failed, ${counts(run)[2]} n/a.`,
    "",
    "Timing movements of 10% or less are flagged as noise, not regressions.",
    "",
    metricsChanges(run, previous),
  ];
  const pillars = new Map(METRICS.map((metric) => [metric.name, String(metric.pillar)]));
  for (const pillar of ["1", "2", "3", "gate", "other"]) {
    const results = run.results.filter(
      (result) => (pillars.get(result.metric) ?? "other") === pillar,
    );
    if (!results.length) continue;
    lines.push(
      pillar === "gate" ? "## Correctness gate" : `## Pillar ${pillar}`,
      "",
      "| Metric | Target | Value | Goal | Status | Note |",
      "| --- | --- | --- | --- | --- | --- |",
    );
    for (const result of results) {
      const before = old.get(targetKey(result));
      const noise =
        before && movement(before.value, result.value).noise ? "Timing change ≤10%: noise." : "";
      lines.push(
        "| " +
          [
            result.metric,
            result.name,
            result.value,
            result.target,
            STATUS_LABEL[result.status].trim(),
            [result.note, noise].filter(Boolean).join(" "),
          ]
            .map(markdownCell)
            .join(" | ") +
          " |",
      );
    }
    lines.push("");
  }
  lines.push(
    "## Recent runs",
    "",
    ...days
      .slice()
      .sort()
      .reverse()
      .slice(0, 28)
      .map((day) => `- [${day}](daily/${day}.json)`),
    "",
  );
  return lines.join("\n");
}

/** Write only the explicit data worktree. No git or network side effects. */
export function storeDailyMetrics(run: MetricsRun, directory: string): string {
  if (
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(run.finished) ||
    !Number.isFinite(Date.parse(run.finished))
  )
    throw new Error("metrics run needs an ISO finished timestamp");
  mkdirSync(join(directory, "daily"), { recursive: true });
  const historyFile = join(directory, "history.tsv");
  const history = existsSync(historyFile) ? readFileSync(historyFile, "utf8") : "";
  const previous = previousMetrics(history, run);
  // One file per run, named by its UTC date and hour, so the 00:00 and
  // 12:00 runs of one day keep separate files.
  const stamp = run.finished.slice(0, 13);
  writeFileSync(join(directory, "daily", stamp + ".json"), JSON.stringify(run, null, 2) + "\n");
  writeFileSync(historyFile, appendMetricsHistory(history, run));
  const days = readdirSync(join(directory, "daily"))
    .filter((name) => /^\d{4}-\d{2}-\d{2}(T\d{2})?\.json$/.test(name))
    .map((name) => name.slice(0, -5));
  writeFileSync(join(directory, "README.md"), metricsReadme(run, previous, days));
  return metricsChanges(run, previous);
}

async function main(): Promise<number> {
  // `pnpm run metrics -- --only X` passes the `--` through; drop it.
  const argv = process.argv.slice(2);
  const { values } = parseArgs({
    args: argv[0] === "--" ? argv.slice(1) : argv,
    options: {
      hd: { type: "string" },
      json: { type: "string" },
      only: { type: "string", multiple: true },
      pillar: { type: "string" },
      list: { type: "boolean" },
      "keep-temp": { type: "boolean" },
      suite: { type: "boolean" },
      max: { type: "boolean" },
      long: { type: "boolean" },
      help: { type: "boolean" },
    },
  });
  if (values.help) {
    console.log(usage());
    return 0;
  }
  if (values.list) {
    console.log(
      formatTable([
        ["metric", "pillar", "measures"],
        ...METRICS.map((m) => [m.name, String(m.pillar), m.summary]),
      ]),
    );
    return 0;
  }
  const selected = selectMetrics(METRICS, values.only ?? [], values.pillar);
  if (selected.length === 0) throw new Error("no metric selected");
  keepTempDirs(values["keep-temp"] ?? false);
  const hd = parseHdCommand(values.hd ?? process.env.HD_METRICS_COMMAND ?? DEFAULT_HD, REPO_ROOT);
  console.log(`hd under test: ${hd.display}`);
  const context = {
    hd,
    repoRoot: REPO_ROOT,
    log: (line: string) => console.error(`  ${line}`),
    suite: values.suite ?? false,
    max: values.max ?? false,
    long: values.long ?? false,
  };
  const results: TargetResult[] = [];
  for (const metric of selected) {
    const started = performance.now();
    console.error(`running ${metric.name} (pillar ${metric.pillar})`);
    let found: TargetResult[];
    try {
      found = await metric.run(context);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      found = [
        {
          metric: metric.name,
          name: "script",
          value: "-",
          target: "completes",
          status: "fail",
          note: message,
        },
      ];
    }
    for (const result of found) console.log(resultLine(result));
    console.error(`  ${metric.name} took ${((performance.now() - started) / 1000).toFixed(1)} s`);
    results.push(...found);
  }
  const rows = results.map((r) => [
    r.metric,
    r.name,
    r.value,
    r.target,
    STATUS_LABEL[r.status].trim(),
  ]);
  console.log(`\n${formatTable([["metric", "target", "value", "goal", "status"], ...rows])}`);
  const count = (status: TargetResult["status"]) =>
    results.filter((r) => r.status === status).length;
  console.log(`\n${count("pass")} passed, ${count("fail")} failed, ${count("n/a")} n/a`);
  if (values.json) {
    const processors = cpus();
    const run: MetricsRun = {
      hd: hd.display,
      commit: execFileSync("git", ["rev-parse", "HEAD"], {
        cwd: REPO_ROOT,
        encoding: "utf8",
      }).trim(),
      finished: new Date().toISOString(),
      results,
      machine: {
        runner: process.env.HD_METRICS_RUNNER ?? `${process.platform}/${process.arch}`,
        cpus: processors.length,
        cpu: processors[0]?.model ?? "unknown",
      },
    };
    writeFileSync(values.json, JSON.stringify(run, null, 2) + "\n");
  }
  return count("fail") > 0 ? 1 : 0;
}

if (import.meta.main) {
  const stop = (): void => {
    killLiveRuns();
    removeAllTempDirs();
    process.exit(130);
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  try {
    process.exitCode = await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    console.error(usage());
    process.exitCode = 2;
  } finally {
    killLiveRuns();
    removeAllTempDirs();
  }
}
