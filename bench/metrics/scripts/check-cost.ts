// check-cost: what overflow and bounds checks cost at run time, with the
// pipeline fixed.
//
// `hd test --release` keeps the test profile, so checks stay on, and
// selects the optimized pipeline (cli.profile.test.release). `hd run
// --release` selects the release profile, which wraps instead of checking,
// in the same optimized pipeline (cli.profile.release,
// cli.profile.pipeline.release). The package (fixtures/check-cost/) sums a
// 1,000-element `List[i64]` by index, with `+`, `*` and `-`, for R rounds,
// in one function: a test case calls it, which `hd test --release` runs
// (checks on), and the executable calls it, which `hd run --release` runs
// (checks off). The package exists at two sizes (R of 100 and 200,100), and
// each side's time is the median of the large copy minus the median of the
// small one, which cancels build and start-up time. Unchecked work under
// 50 ms is within noise and fails.
// Target (Pillar 1): checked ≤ 1.3x unchecked.
// n/a: when `hd help test` names no `--release` flag, as with an
// implementation that has no optimized pipeline.

import { writeProgram } from "../lib/fixtures.ts";
import { supportsFlag, timedSeries } from "../lib/hd.ts";
import { errorText, failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "check-cost";
const RUNS = 3;
const NOISE_MS = 50;
const TIMEOUT_MS = 120_000;
const SMALL = 100;
const LARGE = 200_100;
const LABEL = "checked run time / unchecked run time";
const TARGET = "≤ 1.30x";

export const checkCost: Metric = {
  name: NAME,
  pillar: 1,
  summary: "run time of a checked-arithmetic loop, checks on vs off, optimized pipeline",
  async run(context) {
    const packages = new Map(
      [SMALL, LARGE].map((rounds) => [
        rounds,
        writeProgram("check-cost", { rounds }, `check-${rounds}`),
      ]),
    );
    if (!(await supportsFlag(context.hd, "test", "--release", packages.get(SMALL)!))) {
      return [notApplicable(NAME, LABEL, TARGET, "hd test has no --release")];
    }
    const time = async (rounds: number, checked: boolean) => {
      const args = checked ? ["test", "--release"] : ["run", "--release"];
      const series = await timedSeries(
        context.hd,
        RUNS,
        () => args,
        { cwd: packages.get(rounds)!, timeoutMs: TIMEOUT_MS },
        `hd ${args.join(" ")} (${rounds} rounds)`,
      );
      if (series.problem) {
        throw new Error(series.problem);
      }
      return p50(series.samples);
    };
    try {
      context.log(`${NAME}: ${RUNS} runs each of 2 sizes x 2 profiles`);
      const checkedSmall = await time(SMALL, true);
      const checkedLarge = await time(LARGE, true);
      const uncheckedSmall = await time(SMALL, false);
      const uncheckedLarge = await time(LARGE, false);
      const checked = checkedLarge - checkedSmall;
      const unchecked = uncheckedLarge - uncheckedSmall;
      if (unchecked < NOISE_MS) {
        return [
          failed(
            NAME,
            LABEL,
            TARGET,
            `unchecked work is within noise (${unchecked.toFixed(1)} ms)`,
          ),
        ];
      }
      return [
        judge(
          NAME,
          LABEL,
          checked / unchecked,
          1.3,
          "x",
          "at-most",
          `checked ${Math.round(checked)} ms, unchecked ${Math.round(unchecked)} ms`,
        ),
      ];
    } catch (error) {
      return [failed(NAME, LABEL, TARGET, errorText(error))];
    }
  },
};
