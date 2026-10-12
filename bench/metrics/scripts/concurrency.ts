// concurrency: many agents checking at once on one machine.
//
// N copies of the small package (one per agent worktree) share one fresh,
// empty HD_CACHE, and N `hd check` processes start at once, each in its own
// copy. N = 1, 4 and 16; 64 too with --max. N = 1 runs 3 times, a fresh
// cache each time. Each process is measured with /usr/bin/time.
// - p95 latency at N against the p95 at N = 1. The target is ≤ 1.5x while
//   N ≤ cores. Past the core count the machine is oversubscribed, so the
//   bound scales: ≤ 1.5x × N / cores (this harness's extension).
// - total CPU of all N processes against N: the growth exponent k of
//   CPU ~ N^k from N = 1 to the largest N, from the median CPU at N = 1.
//   Sublinear means k < 1: a shared cache lets later processes reuse
//   earlier work.
// Targets (Pillar 2): ≤ 1.5x at N = cores; total CPU sublinear in N.
// n/a: the CPU line, when there is no /usr/bin/time.

import { availableParallelism } from "node:os";

import { copyPackage, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { canMeasure, runHd, type RunResult } from "../lib/hd.ts";
import type { MetricContext } from "../lib/metric.ts";
import {
  failed,
  formatValue,
  judge,
  judgeBool,
  notApplicable,
  type Metric,
  type TargetResult,
} from "../lib/metric.ts";
import { growthExponent, p50, p95 } from "../lib/stats.ts";
import { removeTempDir } from "../lib/tmp.ts";

const NAME = "concurrency";
const TIMEOUT_MS = 60_000;
const SOLO_ROUNDS = 3;
const RATIO = 1.5;

/** The p95 latency bound at N processes on `cores` cores, as a ratio to N = 1. */
export const latencyBound = (n: number, cores: number): number =>
  n <= cores ? RATIO : (RATIO * n) / cores;

/** Starts N checks at once, each in its own copy, sharing one fresh cache. */
async function round(context: MetricContext, source: string, n: number) {
  const env = freshCache();
  const dirs = Array.from({ length: n }, () => copyPackage(source, NAME));
  const results = await Promise.all(
    dirs.map((cwd) =>
      runHd(context.hd, ["check"], { cwd, env, timeoutMs: TIMEOUT_MS, measure: true }),
    ),
  );
  for (const dir of dirs) removeTempDir(dir);
  const problems = results
    .map((result, index) => runProblem(result, TIMEOUT_MS, `check ${index + 1} of ${n}`))
    .filter((problem) => problem !== undefined);
  return { results, problem: problems[0] };
}

const totalCpu = (results: readonly RunResult[]): number =>
  results.reduce((sum, result) => sum + (result.cpuMs ?? Number.NaN), 0);

export const concurrency: Metric = {
  name: NAME,
  pillar: 2,
  summary: "N = 1, 4, 16 (64 with --max) concurrent hd check: p95 latency and total CPU",
  async run(context) {
    const cores = availableParallelism();
    const counts = context.max ? [4, 16, 64] : [4, 16];
    const { dir: source } = materialize("small", NAME);
    const cpuLabel = `total CPU growth exponent, N = 1 to ${counts.at(-1)}`;
    const ratioLabel = (n: number) => `p95 latency at N = ${n} vs N = 1`;
    const solo: RunResult[] = [];
    for (let index = 0; index < SOLO_ROUNDS; index++) {
      const { results, problem } = await round(context, source, 1);
      if (problem)
        return [
          ...counts.map((n) =>
            failed(NAME, ratioLabel(n), `≤ ${formatValue(latencyBound(n, cores), "x")}`, problem),
          ),
          failed(NAME, cpuLabel, "< 1.00", problem),
        ];
      solo.push(...results);
    }
    const soloP95 = p95(solo.map((result) => result.wallMs));
    const soloCpu = p50(solo.map((result) => result.cpuMs ?? Number.NaN));
    const out: TargetResult[] = [];
    let last: { readonly n: number; readonly cpu: number } | undefined;
    let lastProblem: string | undefined;
    for (const n of counts) {
      context.log(`${NAME}: ${n} concurrent checks on ${cores} cores`);
      const bound = latencyBound(n, cores);
      const { results, problem } = await round(context, source, n);
      if (problem) {
        out.push(failed(NAME, ratioLabel(n), `≤ ${formatValue(bound, "x")}`, problem));
        lastProblem = problem;
        continue;
      }
      const ratio = p95(results.map((result) => result.wallMs)) / soloP95;
      const cpu = totalCpu(results);
      const note = `p95 ${formatValue(soloP95, "ms")} at N = 1; ${cores} cores; total CPU ${formatValue(cpu, "ms")}`;
      out.push(judge(NAME, ratioLabel(n), ratio, bound, "x", "at-most", note));
      if (n === counts.at(-1)) last = { n, cpu };
    }
    if (!canMeasure()) {
      out.push(notApplicable(NAME, cpuLabel, "< 1.00", "no /usr/bin/time on this host"));
      return out;
    }
    if (!last) {
      out.push(failed(NAME, cpuLabel, "< 1.00", lastProblem ?? "the largest N did not run"));
      return out;
    }
    const k = growthExponent({ size: 1, cost: soloCpu }, { size: last.n, cost: last.cpu });
    out.push(
      judgeBool(
        NAME,
        cpuLabel,
        Number.isFinite(k) && k < 1,
        "< 1.00 (sublinear)",
        formatValue(k, "exponent"),
        `${formatValue(soloCpu, "ms")} at N = 1, ${formatValue(last.cpu, "ms")} at N = ${last.n}`,
      ),
    );
    return out;
  },
};
