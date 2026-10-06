// pathological: compile-time stress cases, each within a time and memory
// budget, with check time growing near-linearly in input size.
//
// Each case of pathological/cases.ts is generated at scales 1, 2 and 4 and
// checked as a single file with `hd check FILE`, measuring wall time and
// peak RSS. Three lines per case:
// - wall time at scale 4 within the case's budget (2 s by default);
// - peak RSS at scale 4 within the case's budget (200 MB by default);
// - the growth exponent k of (time - start-up) ~ size^k from scale 2 to 4,
//   where start-up is the median check of a one-line program; k ≤ 1.3 is
//   near-linear. When the work beyond start-up stays under 100 ms at
//   scale 4, growth is within noise and passes.
// A run over 30 s is stopped; larger scales of that case are skipped.
// The peak RSS needs /usr/bin/time; without it that line is n/a.
// No other n/a case: every hd has `hd check FILE`.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { CASES, type PathologicalCase } from "../pathological/cases.ts";
import { firstLines } from "../lib/fixture.ts";
import { canMeasure, runHd, type RunResult } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { growthExponent, p50 } from "../lib/stats.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "pathological";
const TIMEOUT_MS = 30_000;
const SCALES = [1, 2, 4] as const;
const GROWTH_LIMIT = 1.3;
const NOISE_MS = 100;

async function checkProgram(
  context: MetricContext,
  label: string,
  text: string,
): Promise<RunResult> {
  const dir = makeTempDir(NAME);
  const file = `${label.replaceAll("-", "_")}.hd`;
  writeFileSync(join(dir, file), text);
  return runHd(context.hd, ["check", file], { cwd: dir, timeoutMs: TIMEOUT_MS, measure: true });
}

async function runCase(context: MetricContext, entry: PathologicalCase, startup: number) {
  const name = entry.name;
  const timeLabel = `${name}: time at 4x`;
  const memoryLabel = `${name}: peak RSS at 4x`;
  const growthLabel = `${name}: growth exponent`;
  const timeTarget = `≤ ${entry.budgetMs} ms`;
  const memoryTarget = `≤ ${(entry.budgetBytes / (1024 * 1024)).toFixed(1)} MB`;
  const runs = new Map<number, RunResult>();
  let problem: string | undefined;
  for (const scale of SCALES) {
    context.log(`${NAME}: ${name} at ${scale}x`);
    const result = await checkProgram(context, name, entry.generate(scale));
    if (result.timedOut) problem = `${scale}x timed out after ${TIMEOUT_MS / 1000} s`;
    else if (result.status !== 0) problem = `${scale}x: hd check failed: ${firstLines(result)}`;
    if (problem) break;
    runs.set(scale, result);
  }
  const results: TargetResult[] = [];
  const largest = runs.get(4);
  if (!largest) {
    results.push(
      failed(NAME, timeLabel, timeTarget, problem!),
      failed(NAME, memoryLabel, memoryTarget, problem!),
    );
  } else {
    results.push(judge(NAME, timeLabel, largest.wallMs, entry.budgetMs, "ms"));
    results.push(
      largest.rssBytes === undefined
        ? notApplicable(NAME, memoryLabel, memoryTarget, "no /usr/bin/time on this host")
        : judge(NAME, memoryLabel, largest.rssBytes, entry.budgetBytes, "MB"),
    );
  }
  const [small, large] = largest ? [2, 4] : [1, 2];
  const from = runs.get(small);
  const to = runs.get(large);
  if (!from || !to) {
    results.push(
      failed(NAME, growthLabel, `≤ ${GROWTH_LIMIT}`, problem ?? "no two scales completed"),
    );
  } else {
    const work = (result: RunResult) => result.wallMs - startup;
    const note = `${small}x→${large}x; ${Math.round(from.wallMs)} → ${Math.round(to.wallMs)} ms, start-up ${Math.round(startup)} ms`;
    results.push(
      work(to) < NOISE_MS
        ? {
            metric: NAME,
            name: growthLabel,
            value: "noise",
            target: `≤ ${GROWTH_LIMIT}`,
            status: "pass",
            note,
          }
        : judge(
            NAME,
            growthLabel,
            growthExponent(
              { size: small, cost: Math.max(work(from), 1) },
              { size: large, cost: work(to) },
            ),
            GROWTH_LIMIT,
            "exponent",
            "at-most",
            problem ? `${note}; ${problem}` : note,
          ),
    );
  }
  return results;
}

export const pathological: Metric = {
  name: NAME,
  pillar: 1,
  summary: "compile-time stress cases: time and memory budgets, near-linear growth",
  async run(context) {
    const startupRuns: number[] = [];
    for (let index = 0; index < 3; index++) {
      const result = await checkProgram(context, "startup", "fn run() -> i32:\n    1\n");
      if (result.status !== 0 || result.timedOut)
        return [
          failed(
            NAME,
            "start-up check",
            "succeeds",
            `hd check of a one-line program failed: ${firstLines(result)}`,
          ),
        ];
      startupRuns.push(result.wallMs);
    }
    const startup = p50(startupRuns);
    if (!canMeasure()) context.log(`${NAME}: no /usr/bin/time; peak RSS is n/a`);
    const results: TargetResult[] = [];
    for (const entry of CASES) results.push(...(await runCase(context, entry, startup)));
    return results;
  },
};
