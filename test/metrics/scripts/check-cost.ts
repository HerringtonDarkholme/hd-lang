// release-check-cost: what overflow and bounds checks cost at run time.
//
// Agents run tests in the debug profile, so its checks slow every test run.
// The CLI gives `hd test` no `--release` (cli.profile.flag-only), so this
// script times an executable instead: `hd run` against `hd run --release`.
// The program sums a 1,000-element `List[i64]` by index, with `+`, `*` and
// `-`, for R rounds. Two copies differ only in R (100 and 200,100), and each
// profile's run time is the median of the large copy minus the median of the
// small one, which cancels build and start-up time. Release work under
// 50 ms is within noise and fails.
// Target (Pillar 1): debug ≤ 1.3x release.
// n/a: when `hd help run` names no `--release` flag.

import { runHd, supportsFlag } from "../lib/hd.ts";
import { runProblem } from "../lib/fixture.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir, writeTree } from "../lib/tmp.ts";

const NAME = "release-check-cost";
const RUNS = 3;
const NOISE_MS = 50;
const TIMEOUT_MS = 120_000;
const SMALL = 100;
const LARGE = 200_100;
const LABEL = "debug run time / release run time";
const TARGET = "≤ 1.30x";

export const workload = (rounds: number): string =>
  [
    "pub fn main() -> void $ Console:",
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
    '    println("${total}")',
    "",
  ].join("\n");

export const releaseCheckCost: Metric = {
  name: NAME,
  pillar: 1,
  summary: "run time of a checked-arithmetic loop, debug vs release (hd run)",
  async run(context) {
    const dirs = new Map<number, string>();
    for (const rounds of [SMALL, LARGE]) {
      const dir = makeTempDir(`release-${rounds}`);
      writeTree(
        dir,
        new Map([
          ["hd.toml", '[package]\nname = "loop"\n'],
          ["src/main.hd", workload(rounds)],
        ]),
      );
      dirs.set(rounds, dir);
    }
    if (!(await supportsFlag(context.hd, "run", "--release", dirs.get(SMALL)!)))
      return [notApplicable(NAME, LABEL, TARGET, "hd run has no --release")];
    const time = async (rounds: number, release: boolean) => {
      const samples: number[] = [];
      let output: string | undefined;
      for (let index = 0; index < RUNS; index++) {
        const args = release ? ["run", "--release"] : ["run"];
        const result = await runHd(context.hd, args, {
          cwd: dirs.get(rounds)!,
          timeoutMs: TIMEOUT_MS,
        });
        const problem = runProblem(result, TIMEOUT_MS, `hd ${args.join(" ")} (${rounds} rounds)`);
        if (problem) throw new Error(problem);
        if (output !== undefined && result.stdout !== output)
          throw new Error("the program's output changed between runs");
        output = result.stdout;
        samples.push(result.wallMs);
      }
      return { ms: p50(samples), output: output! };
    };
    try {
      context.log(`${NAME}: ${RUNS} runs each of 2 sizes x 2 profiles`);
      const debugSmall = await time(SMALL, false);
      const debugLarge = await time(LARGE, false);
      const releaseSmall = await time(SMALL, true);
      const releaseLarge = await time(LARGE, true);
      if (debugLarge.output !== releaseLarge.output)
        return [failed(NAME, LABEL, TARGET, "debug and release print different totals")];
      const debug = debugLarge.ms - debugSmall.ms;
      const release = releaseLarge.ms - releaseSmall.ms;
      if (release < NOISE_MS)
        return [
          failed(NAME, LABEL, TARGET, `release work is within noise (${release.toFixed(1)} ms)`),
        ];
      return [
        judge(
          NAME,
          LABEL,
          debug / release,
          1.3,
          "x",
          "at-most",
          `debug ${Math.round(debug)} ms, release ${Math.round(release)} ms`,
        ),
      ];
    } catch (error) {
      return [failed(NAME, LABEL, TARGET, error instanceof Error ? error.message : String(error))];
    }
  },
};
