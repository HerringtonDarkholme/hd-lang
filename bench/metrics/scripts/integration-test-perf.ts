// integration-test-perf: what each integration test program and doc test
// costs a test run.
//
// Integration tests: packages with 0, 2 and 10 programs under `tests/`
// (lib/gen.ts, from fixtures/integration-test-perf/).
// Each program has 2 test cases that write a file into `temp_dir()`, under
// the default profile's real file system, and read it back. After a
// warm-up, `hd test` runs RUNS times in each package, interleaved.
// - setup per test program: the median of (10-program run i - 2-program
//   run i) / 8, which includes its 2 cases;
// - growth: total time beyond the 0-program package grows sublinearly in
//   programs when the programs share a build: the exponent k of
//   (T(10) - T(0)) / (T(2) - T(0)) = 5^k is below 1.
// Doc tests: packages with 2 and 10 doc tests, each its own program; the
// cost per doc test is the median of (10 run i - 2 run i) / 8.
// Targets (Pillar 3): ≤ 20 ms setup per test program, and per doc test
// (this harness applies the program budget, since each doc test is its own
// program); total sublinear in programs.
// n/a: never; a suite that fails to run fails its line.

import { passedCount, timeCommand } from "../lib/artifact.ts";
import type { MetricContext, TargetResult } from "../lib/metric.ts";
import { writePackage } from "../lib/fixtures.ts";
import { docTestPackage, integrationPackage } from "../lib/gen.ts";
import { errorText, failed, formatValue, judge, judgeBool, type Metric } from "../lib/metric.ts";
import { growthExponent, p50 } from "../lib/stats.ts";

const NAME = "integration-test-perf";
const RUNS = 3;
const TIMEOUT_MS = 300_000;
const SETUP_LIMIT_MS = 20;
const FEW = 2;
const MANY = 10;

const setupTarget = `≤ ${formatValue(SETUP_LIMIT_MS, "ms")}`;
const GROWTH_TARGET = "exponent < 1 (sublinear)";

async function integration(context: MetricContext): Promise<TargetResult[]> {
  const labels = ["setup per integration test program", "total time growth in programs"] as const;
  try {
    const timing = await timeCommand(
      context.hd,
      [0, FEW, MANY].map((programs) =>
        writePackage(`integration-${programs}`, integrationPackage(programs)),
      ),
      ["test"],
      { runs: RUNS, timeoutMs: TIMEOUT_MS, label: "the integration tests" },
    );
    const [none, few, many] = timing.samples as [number[], number[], number[]];
    const setup = p50(many.map((ms, index) => ms - few[index]!)) / (MANY - FEW);
    const beyond = (samples: number[]) => p50(samples.map((ms, index) => ms - none[index]!));
    const k = growthExponent({ size: FEW, cost: beyond(few) }, { size: MANY, cost: beyond(many) });
    const tests = 2 * MANY + 1;
    return [
      judge(
        NAME,
        labels[0],
        Math.max(setup, 0),
        SETUP_LIMIT_MS,
        "ms",
        "at-most",
        `${MANY} programs: ${formatValue(p50(many), "ms")} for ${tests} tests, ${formatValue(p50(many) / tests, "ms")} per test`,
      ),
      judgeBool(
        NAME,
        labels[1],
        k < 1,
        GROWTH_TARGET,
        Number.isNaN(k) ? "-" : k.toFixed(3),
        `beyond the 0-program run: ${formatValue(beyond(few), "ms")} at ${FEW}, ${formatValue(beyond(many), "ms")} at ${MANY} programs`,
      ),
    ];
  } catch (error) {
    return [
      failed(NAME, labels[0], setupTarget, errorText(error)),
      failed(NAME, labels[1], GROWTH_TARGET, errorText(error)),
    ];
  }
}

async function docTests(context: MetricContext): Promise<TargetResult> {
  const label = "cost per doc test";
  try {
    const timing = await timeCommand(
      context.hd,
      [FEW, MANY].map((count) => writePackage(`doc-${count}`, docTestPackage(count))),
      ["test"],
      { runs: RUNS, timeoutMs: TIMEOUT_MS, label: "the doc tests" },
    );
    const [few, many] = timing.samples as [number[], number[]];
    // The library's unit test runs too.
    const ran = `; the run reports ${passedCount(timing.outputs[1]!)} of ${MANY + 1} passed`;
    return judge(
      NAME,
      label,
      Math.max(p50(many.map((ms, index) => ms - few[index]!)) / (MANY - FEW), 0),
      SETUP_LIMIT_MS,
      "ms",
      "at-most",
      `${MANY} doc tests: ${formatValue(p50(many), "ms")}${ran}`,
    );
  } catch (error) {
    return failed(NAME, label, setupTarget, errorText(error));
  }
}

export const integrationTestPerf: Metric = {
  name: NAME,
  pillar: 3,
  summary: "setup cost per integration test program and per doc test; growth in programs",
  async run(context) {
    context.log(`${NAME}: 0, ${FEW} and ${MANY} test programs; ${FEW} and ${MANY} doc tests`);
    return [...(await integration(context)), await docTests(context)];
  },
};
