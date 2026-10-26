// unit-test-perf: what each unit test costs a test run.
//
// A generated package of 10 modules (lib/gen.ts, from
// fixtures/unit-test-perf/). Each module has a pure function and half its
// tests in its `tests:` block; the other half are in its test module
// `src/mK_test.hd`. No test uses a host capability. Two packages
// hold 100 and 1,000 tests. After a warm-up run, `hd test` runs RUNS times
// in each, interleaved:
// - 1,000 tests, warm: the median run of the large package;
// - overhead per test: the median of (large run i - small run i) / 900.
// Targets (Pillar 3): ≤ 1 ms per test; 1,000 unit tests ≤ 1 s.
// n/a: never; a suite that fails to run fails its line.

import { passedCount, timeCommand } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { writePackage } from "../lib/fixtures.ts";
import { unitPackage } from "../lib/gen.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "unit-test-perf";
const SMALL = 100;
const LARGE = 1_000;
const RUNS = 3;
const TIMEOUT_MS = 300_000;
const PER_TEST_LIMIT_MS = 1;
const TOTAL_LIMIT_MS = 1_000;

export const unitTestPerf: Metric = {
  name: NAME,
  pillar: 3,
  summary: "hd test of 1,000 generated unit tests, warm, and the overhead per test",
  async run(context) {
    const labels = ["1,000 unit tests, warm", "overhead per unit test"] as const;
    const dirs = [LARGE, SMALL].map((tests) => writePackage(`units-${tests}`, unitPackage(tests)));
    context.log(`${NAME}: hd test of ${LARGE} and ${SMALL} unit tests, ${RUNS} runs each`);
    try {
      const timing = await timeCommand(context.hd, dirs, ["test"], {
        runs: RUNS,
        timeoutMs: TIMEOUT_MS,
        label: "the unit tests",
      });
      const [large, small] = timing.samples as [number[], number[]];
      const note = `the run reports ${passedCount(timing.outputs[0]!)} of ${LARGE} passed`;
      const results: TargetResult[] = [
        judge(NAME, labels[0], p50(large), TOTAL_LIMIT_MS, "ms", "at-most", note),
        judge(
          NAME,
          labels[1],
          Math.max(p50(large.map((ms, index) => ms - small[index]!)) / (LARGE - SMALL), 0),
          PER_TEST_LIMIT_MS,
          "ms",
          "at-most",
          `${SMALL} tests: ${formatValue(p50(small), "ms")}`,
        ),
      ];
      return results;
    } catch (error) {
      return [
        failed(NAME, labels[0], `≤ ${formatValue(TOTAL_LIMIT_MS, "ms")}`, errorText(error)),
        failed(NAME, labels[1], `≤ ${formatValue(PER_TEST_LIMIT_MS, "ms")}`, errorText(error)),
      ];
    }
  },
};
