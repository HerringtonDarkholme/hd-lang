// The metric runner: runs the selected metric scripts against one `hd` and
// judges each target. See README.md.
//
// Usage: node --experimental-strip-types test/metrics/run.ts
//          [--hd "COMMAND"] [--only NAME[,NAME...]] [--pillar N] [--list] [--keep-temp]
//          [--suite] [--max] [--long]
//
// It prints one line per target as it is judged, then a summary table, and
// exits 1 when any target fails. An n/a target never fails the run.

import { join } from "node:path";
import { parseArgs } from "node:util";

import { killLiveRuns, parseHdCommand } from "./lib/hd.ts";
import type { Metric, TargetResult } from "./lib/metric.ts";
import { keepTempDirs, removeAllTempDirs } from "./lib/tmp.ts";
import { METRICS } from "./scripts/index.ts";

const REPO_ROOT = join(import.meta.dirname, "..", "..");
const DEFAULT_HD = "node --experimental-strip-types bin/hd.js";

const STATUS_LABEL = { pass: "PASS", fail: "FAIL", "n/a": "N/A " } as const;

function usage(): string {
  return [
    "usage: run.ts [--hd COMMAND] [--only NAME[,NAME...]] [--pillar N] [--list] [--keep-temp]",
    "              [--suite] [--max] [--long]",
    "",
    `--hd COMMAND   the hd under test (default: ${DEFAULT_HD}, from the repository root)`,
    "--only NAMES   run only these metrics; repeatable or comma-separated",
    "--pillar N     run only the metrics of pillar N (1, 2, 3, or gate)",
    "--list         list the metrics and exit",
    "--keep-temp    keep temporary directories, for debugging",
    "--suite        also run the metrics that take minutes (suite-cpu, conformance)",
    "--max          also run the largest scales (64 concurrent checks)",
    "--long         run long-run-memory for 10 minutes instead of 60 s",
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

async function main(): Promise<number> {
  // `pnpm run metrics -- --only X` passes the `--` through; drop it.
  const argv = process.argv.slice(2);
  const { values } = parseArgs({
    args: argv[0] === "--" ? argv.slice(1) : argv,
    options: {
      hd: { type: "string" },
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
