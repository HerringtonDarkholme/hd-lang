// proptest-perf: how many property-test cases a test run affords, and how
// fast a failure shrinks.
//
// The suite is user-style code, in fixtures/proptest-perf/: an `Item`
// record, a `Shape` enum with payloads, and an `Order` that nests both in a
// list, each deriving `Arbitrary`, under 3 `it_prop` properties that hold. Two copies differ
// only in each property's `cases` option, SMALL and LARGE. After a
// warm-up, `hd test` runs RUNS times in each, interleaved, every run in a
// fresh copy of its package; cases per second are 3 x (LARGE - SMALL)
// over the median of (large run i - small run i).
// Shrinking: one property fails for a price of 1,000 or more. Two copies
// differ in its `shrink` option, 0 and the default 500, and the shrink
// time is the median of (shrinking run i - non-shrinking run i). Both
// runs use one seed when `hd help test` names `--seed`. A fresh copy per
// run keeps a saved regression stream from replaying into the next run.
// Targets (Pillar 3): ≥ 100k cases/s for simple generators; shrink ≤ 1 s.
// n/a: never; a suite that fails to run fails its line.

import { timeCommand } from "../lib/artifact.ts";
import { writeProgram } from "../lib/fixtures.ts";
import { supportsFlag } from "../lib/hd.ts";
import type { MetricContext, TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "proptest-perf";
const SMALL = 100;
const LARGE = 2_100;
const PROPERTIES = 3;
const RUNS = 3;
const TIMEOUT_MS = 300_000;
const CASES_LIMIT = 100_000;
const SHRINK_LIMIT_MS = 1_000;
const SEED = "7";

/** A fresh copy of the passing suite, with `cases` for each property. */
const suite = (cases: number): string =>
  writeProgram("proptest-perf/suite", { cases }, `prop-${cases}`);

/** A fresh copy of the failing suite, with `shrink` steps for its property. */
const failing = (shrink: number): string =>
  writeProgram("proptest-perf/failing", { shrink }, `shrink-${shrink}`);

async function casesPerSecond(context: MetricContext, seed: string[]): Promise<TargetResult> {
  const label = "property cases per second";
  try {
    const timing = await timeCommand(context.hd, [suite(LARGE), suite(SMALL)], ["test", ...seed], {
      runs: RUNS,
      timeoutMs: TIMEOUT_MS,
      label: "the property suite",
      fresh: true,
    });
    const [large, small] = timing.samples as [number[], number[]];
    const ms = p50(large.map((value, index) => value - small[index]!));
    const cases = PROPERTIES * (LARGE - SMALL);
    if (ms <= 0) throw new Error("no work measured between the two case counts");
    return judge(
      NAME,
      label,
      cases / (ms / 1000),
      CASES_LIMIT,
      "per-s",
      "at-least",
      `${cases} extra cases took ${formatValue(ms, "ms")}; hd test, median of ${RUNS}`,
    );
  } catch (error) {
    return failed(NAME, label, `≥ ${formatValue(CASES_LIMIT, "per-s")}`, errorText(error));
  }
}

async function shrinkTime(context: MetricContext, seed: string[]): Promise<TargetResult> {
  const label = "time to shrink a known failure";
  try {
    const timing = await timeCommand(context.hd, [failing(500), failing(0)], ["test", ...seed], {
      runs: RUNS,
      timeoutMs: TIMEOUT_MS,
      label: "the failing property",
      expectFailure: true,
      fresh: true,
    });
    const [shrinking, plain] = timing.samples as [number[], number[]];
    const ms = Math.max(p50(shrinking.map((value, index) => value - plain[index]!)), 0);
    const reported = timing.outputs[0]!.includes("prices stay under 1000");
    return judge(
      NAME,
      label,
      ms,
      SHRINK_LIMIT_MS,
      "ms",
      "at-most",
      `shrink=500 minus shrink=0, median of ${RUNS}${reported ? "" : "; the failure did not name the property"}`,
    );
  } catch (error) {
    return failed(NAME, label, `≤ ${formatValue(SHRINK_LIMIT_MS, "ms")}`, errorText(error));
  }
}

export const proptestPerf: Metric = {
  name: NAME,
  pillar: 3,
  summary: "property-test cases per second with derived Arbitrary, and time to shrink a failure",
  async run(context) {
    const seed = (await supportsFlag(context.hd, "test", "--seed", makeTempDir("seed-probe")))
      ? ["--seed", SEED]
      : [];
    context.log(`${NAME}: ${PROPERTIES} properties at ${SMALL} and ${LARGE} cases, then a shrink`);
    return [await casesPerSecond(context, seed), await shrinkTime(context, seed)];
  },
};
