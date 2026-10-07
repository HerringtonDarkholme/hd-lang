// startup: the fixed cost of starting hd.
//
// Two commands, each run once to warm the OS file cache, then 5 times with
// /usr/bin/time; the values are medians:
// - the version command, `hd --version` or `hd version`, whichever exits 0;
//   with neither, `hd help`, and the line says so;
// - `hd check empty.hd` of an empty file outside any package.
// Each command has three lines: wall time, CPU time and peak RSS.
// Targets (Pillar 2): ≤ 20 ms (wall and CPU), ≤ 10 MB.
// n/a: the CPU and RSS lines, when there is no /usr/bin/time.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { runProblem } from "../lib/fixture.ts";
import { canMeasure, runHd } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "startup";
const RUNS = 5;
const TIMEOUT_MS = 10_000;
const TIME_LIMIT_MS = 20;
const RSS_LIMIT = 10 * 1024 * 1024;

/** The first of `hd --version` and `hd version` that exits 0, else `hd help`. */
async function versionCommand(context: MetricContext, cwd: string) {
  for (const args of [["--version"], ["version"]]) {
    const result = await runHd(context.hd, args, { cwd, timeoutMs: TIMEOUT_MS });
    if (result.status === 0) return { args, label: `hd ${args[0]}` };
  }
  return { args: ["help"], label: "hd help (no version command)" };
}

async function measure(
  context: MetricContext,
  label: string,
  args: readonly string[],
  cwd: string,
): Promise<TargetResult[]> {
  const labels = [`${label}: wall time`, `${label}: CPU time`, `${label}: peak RSS`];
  const targets = [`≤ ${TIME_LIMIT_MS} ms`, `≤ ${TIME_LIMIT_MS} ms`, "≤ 10.0 MB"];
  const walls: number[] = [];
  const cpus: number[] = [];
  const rsss: number[] = [];
  for (let index = 0; index <= RUNS; index++) {
    const result = await runHd(context.hd, args, { cwd, timeoutMs: TIMEOUT_MS, measure: true });
    const problem = runProblem(result, TIMEOUT_MS, `${label}, run ${index + 1}`);
    if (problem) return labels.map((name, k) => failed(NAME, name, targets[k]!, problem));
    if (index === 0) continue;
    walls.push(result.wallMs);
    if (result.cpuMs !== undefined) cpus.push(result.cpuMs);
    if (result.rssBytes !== undefined) rsss.push(result.rssBytes);
  }
  const unmeasured = (k: number) =>
    notApplicable(NAME, labels[k]!, targets[k]!, "no /usr/bin/time on this host");
  return [
    judge(NAME, labels[0]!, p50(walls), TIME_LIMIT_MS, "ms"),
    cpus.length ? judge(NAME, labels[1]!, p50(cpus), TIME_LIMIT_MS, "ms") : unmeasured(1),
    rsss.length ? judge(NAME, labels[2]!, p50(rsss), RSS_LIMIT, "MB") : unmeasured(2),
  ];
}

export const startup: Metric = {
  name: NAME,
  pillar: 2,
  summary: "wall time, CPU time and peak RSS of the version command and of checking an empty file",
  async run(context) {
    const cwd = makeTempDir(NAME);
    writeFileSync(join(cwd, "empty.hd"), "");
    if (!canMeasure()) context.log(`${NAME}: no /usr/bin/time; CPU and RSS are n/a`);
    const version = await versionCommand(context, cwd);
    return [
      ...(await measure(context, version.label, version.args, cwd)),
      ...(await measure(context, "hd check empty.hd", ["check", "empty.hd"], cwd)),
    ];
  },
};
