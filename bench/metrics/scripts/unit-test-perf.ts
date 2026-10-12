// unit-test-perf: what each unit test costs a test run.
//
// A generated package of 10 modules. Each module has a pure function and
// half its tests in its `tests:` block; the other half are in its test
// module `src/mK_test.hd`. No test uses a host capability. Two packages
// hold 100 and 1,000 tests. After a warm-up run, `hd test` runs RUNS times
// in each, interleaved:
// - 1,000 tests, warm: the median run of the large package;
// - overhead per test: the median of (large run i - small run i) / 900.
// Targets (Pillar 3): ≤ 1 ms per test; 1,000 unit tests ≤ 1 s.
// n/a: never; a suite that fails to run fails its line.

import { passedCount, timeCommand } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { failed, formatValue, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir, writeTree } from "../lib/tmp.ts";

const NAME = "unit-test-perf";
const MODULES = 10;
const SMALL = 100;
const LARGE = 1_000;
const RUNS = 3;
const TIMEOUT_MS = 300_000;
const PER_TEST_LIMIT_MS = 1;
const TOTAL_LIMIT_MS = 1_000;

/** The files of a package of `tests` unit tests over MODULES modules. */
export function unitPackage(tests: number): Map<string, string> {
  const files = new Map<string, string>([["hd.toml", '[package]\nname = "units"\n']]);
  const perModule = tests / MODULES;
  const half = Math.ceil(perModule / 2);
  for (let k = 0; k < MODULES; k++) {
    const factor = k + 2;
    const expected = (j: number) => j * factor + 1;
    const block = Array.from({ length: half }, (_, j) => [
      `    it("scale${k} block case ${j}"):`,
      `        assert_equal(scale${k}(${j}), ${expected(j)}, reason="linear")`,
    ]).flat();
    files.set(
      `src/m${k}.hd`,
      [
        "use std.testing.assert_equal",
        "",
        `pub fn scale${k}(x: i64) -> i64:`,
        `    x * ${factor} + 1`,
        "",
        "tests:",
        ...block,
        "",
      ].join("\n"),
    );
    const rest = Array.from({ length: perModule - half }, (_, index) => {
      const j = half + index;
      return [
        `it("scale${k} module case ${j}"):`,
        `    assert_equal(scale${k}(${j}), ${expected(j)}, reason="linear")`,
        "",
      ];
    }).flat();
    files.set(
      `src/m${k}_test.hd`,
      [`use pkg.m${k}.{scale${k}}`, "use std.testing.assert_equal", "", ...rest].join("\n"),
    );
  }
  return files;
}

export const unitTestPerf: Metric = {
  name: NAME,
  pillar: 3,
  summary: "hd test of 1,000 generated unit tests, warm, and the overhead per test",
  async run(context) {
    const labels = ["1,000 unit tests, warm", "overhead per unit test"] as const;
    const dirs = [LARGE, SMALL].map((tests) => {
      const dir = makeTempDir(`units-${tests}`);
      writeTree(dir, unitPackage(tests));
      return dir;
    });
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
      const reason = error instanceof Error ? error.message : String(error);
      return [
        failed(NAME, labels[0], `≤ ${formatValue(TOTAL_LIMIT_MS, "ms")}`, reason),
        failed(NAME, labels[1], `≤ ${formatValue(PER_TEST_LIMIT_MS, "ms")}`, reason),
      ];
    }
  },
};
