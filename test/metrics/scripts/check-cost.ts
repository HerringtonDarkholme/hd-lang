// check-cost: what overflow and bounds checks cost at run time, with the
// pipeline fixed.
//
// `hd test --release` keeps the test profile, so checks stay on, and
// selects the optimized pipeline (cli.profile.test.release). `hd run
// --release` selects the release profile, which wraps instead of checking,
// in the same optimized pipeline (cli.profile.release,
// cli.profile.pipeline.release). The program sums a 1,000-element
// `List[i64]` by index, with `+`, `*` and `-`, for R rounds: once as a test
// case that `hd test --release` runs (checks on), and once as an executable
// that `hd run --release` runs (checks off). Each copy exists at two sizes
// (R of 100 and 200,100), and each side's time is the median of the large
// copy minus the median of the small one, which cancels build and start-up
// time. Unchecked work under 50 ms is within noise and fails.
// Target (Pillar 1): checked ≤ 1.3x unchecked.
// n/a: when `hd help test` names no `--release` flag, as with an
// implementation that has no optimized pipeline.

import { runHd, supportsFlag } from "../lib/hd.ts";
import { runProblem } from "../lib/fixture.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir, writeTree } from "../lib/tmp.ts";

const NAME = "check-cost";
const RUNS = 3;
const NOISE_MS = 50;
const TIMEOUT_MS = 120_000;
const SMALL = 100;
const LARGE = 200_100;
const LABEL = "checked run time / unchecked run time";
const TARGET = "≤ 1.30x";

const loop = (rounds: number): string[] => [
  "    let values: mut List[i64] = []",
  "    for i in +0..1000:",
  "        values.push(i64(i))",
  "    let total: i64 = 0",
  "    let round: i64 = 0",
  `    while round < ${rounds}:`,
  "        let j: usize = 0",
  "        while j < values.len():",
  "            total = total + values[j] * 3 - round",
  "            j = j + 1",
  "        round = round + 1",
];

/** The executable: the loop in `main`, printing its total. */
export const executable = (rounds: number): string =>
  ["pub fn main() -> void $ Console:", ...loop(rounds), '    println("${total}")', ""].join("\n");

/** The test case: the same loop in a function that one test calls. */
export const testCase = (rounds: number): string =>
  [
    "use std.testing.assert",
    "",
    "fn sum_loop() -> i64:",
    ...loop(rounds),
    "    total",
    "",
    "tests:",
    '    it("sums by index"):',
    '        assert(sum_loop() != 1, reason="the loop ran")',
    "",
  ].join("\n");

const packageWith = (label: string, source: string): string => {
  const dir = makeTempDir(label);
  writeTree(
    dir,
    new Map([
      ["hd.toml", '[package]\nname = "loop"\n'],
      ["src/main.hd", source],
    ]),
  );
  return dir;
};

export const checkCost: Metric = {
  name: NAME,
  pillar: 1,
  summary: "run time of a checked-arithmetic loop, checks on vs off, optimized pipeline",
  async run(context) {
    const tests = new Map<number, string>();
    const programs = new Map<number, string>();
    for (const rounds of [SMALL, LARGE]) {
      tests.set(rounds, packageWith(`check-test-${rounds}`, testCase(rounds)));
      programs.set(rounds, packageWith(`check-run-${rounds}`, executable(rounds)));
    }
    if (!(await supportsFlag(context.hd, "test", "--release", tests.get(SMALL)!)))
      return [notApplicable(NAME, LABEL, TARGET, "hd test has no --release")];
    const time = async (rounds: number, checked: boolean) => {
      const samples: number[] = [];
      const args = checked ? ["test", "--release"] : ["run", "--release"];
      const cwd = (checked ? tests : programs).get(rounds)!;
      for (let index = 0; index < RUNS; index++) {
        const result = await runHd(context.hd, args, { cwd, timeoutMs: TIMEOUT_MS });
        const problem = runProblem(result, TIMEOUT_MS, `hd ${args.join(" ")} (${rounds} rounds)`);
        if (problem) throw new Error(problem);
        samples.push(result.wallMs);
      }
      return p50(samples);
    };
    try {
      context.log(`${NAME}: ${RUNS} runs each of 2 sizes x 2 profiles`);
      const checkedSmall = await time(SMALL, true);
      const checkedLarge = await time(LARGE, true);
      const uncheckedSmall = await time(SMALL, false);
      const uncheckedLarge = await time(LARGE, false);
      const checked = checkedLarge - checkedSmall;
      const unchecked = uncheckedLarge - uncheckedSmall;
      if (unchecked < NOISE_MS)
        return [
          failed(NAME, LABEL, TARGET, `unchecked work is within noise (${unchecked.toFixed(1)} ms)`),
        ];
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
      return [failed(NAME, LABEL, TARGET, error instanceof Error ? error.message : String(error))];
    }
  },
};
