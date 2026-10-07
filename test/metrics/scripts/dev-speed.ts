// dev-speed: how slow code from the dev pipeline may run, with the profile
// fixed.
//
// The cases are the `runtime` micro cases (test/perf/micro), each turned
// into a test case: the case's `main` becomes a plain function that a
// `tests:` block calls. `hd test` may use the dev pipeline, and `hd test
// --release` uses the optimized one; both keep the test profile
// (cli.profile.test, cli.profile.test.release, cli.profile.pipeline.release).
// A case's time in a pipeline is its median wall time minus the median of
// an empty test, which cancels build and start-up time. Optimized work
// under 50 ms is within noise and fails.
// Targets (Pillar 1): dev ≤ 4x optimized, geomean; no case over 10x.
// n/a: when `hd help test` names no `--release` flag, as with an
// implementation that has no separate pipelines.

import { runHd, supportsFlag } from "../lib/hd.ts";
import { runProblem } from "../lib/fixture.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { geomean, p50 } from "../lib/stats.ts";
import { makeTempDir, writeTree } from "../lib/tmp.ts";
import { MICRO_CASES, microSource } from "./runtime.ts";

const NAME = "dev-speed";
const RUNS = 3;
const NOISE_MS = 50;
const TIMEOUT_MS = 120_000;
const GEOMEAN_LIMIT = 4;
const CASE_LIMIT = 10;
const GEOMEAN_LABEL = "geomean dev / optimized";
const WORST_LABEL = "worst case dev / optimized";

/** A micro case as test source: its `main` is called by one test case. */
export function asTestCase(source: string): string {
  if (!source.includes("pub fn main() -> void:")) throw new Error("not a micro case program");
  return `${source.replace("pub fn main() -> void:", "fn case_main() -> void:").trimEnd()}

tests:
    it("runs the case"):
        case_main()
`;
}

/** A test with no work, to subtract build and start-up time. */
export const EMPTY_TEST = 'tests:\n    it("empty"):\n        pass\n';

const packageWith = (label: string, source: string): string => {
  const dir = makeTempDir(label);
  writeTree(
    dir,
    new Map([
      ["hd.toml", '[package]\nname = "case"\n'],
      ["src/main.hd", source],
    ]),
  );
  return dir;
};

export const devSpeed: Metric = {
  name: NAME,
  pillar: 1,
  summary: "run time of the micro cases as tests, dev pipeline vs optimized pipeline",
  async run(context) {
    const empty = packageWith("dev-empty", EMPTY_TEST);
    const geomeanTarget = `≤ ${GEOMEAN_LIMIT.toFixed(2)}x`;
    const worstTarget = `≤ ${CASE_LIMIT.toFixed(2)}x`;
    if (!(await supportsFlag(context.hd, "test", "--release", empty)))
      return [
        notApplicable(NAME, GEOMEAN_LABEL, geomeanTarget, "hd test has no --release"),
        notApplicable(NAME, WORST_LABEL, worstTarget, "hd test has no --release"),
      ];
    const time = async (dir: string, optimized: boolean) => {
      const samples: number[] = [];
      const args = optimized ? ["test", "--release"] : ["test"];
      for (let index = 0; index < RUNS; index++) {
        const result = await runHd(context.hd, args, { cwd: dir, timeoutMs: TIMEOUT_MS });
        const problem = runProblem(result, TIMEOUT_MS, `hd ${args.join(" ")}`);
        if (problem) throw new Error(problem);
        samples.push(result.wallMs);
      }
      return p50(samples);
    };
    try {
      context.log(`${NAME}: ${MICRO_CASES.length} cases, ${RUNS} runs each in 2 pipelines`);
      const emptyDev = await time(empty, false);
      const emptyOptimized = await time(empty, true);
      const ratios: { name: string; ratio: number }[] = [];
      const noisy: string[] = [];
      for (const name of MICRO_CASES) {
        const dir = packageWith(`dev-${name}`, asTestCase(microSource(context, name)));
        const dev = (await time(dir, false)) - emptyDev;
        const optimized = (await time(dir, true)) - emptyOptimized;
        if (optimized < NOISE_MS) noisy.push(`${name} (${optimized.toFixed(1)} ms)`);
        else ratios.push({ name, ratio: dev / optimized });
      }
      if (noisy.length > 0) {
        const reason = `optimized work is within noise: ${noisy.join(", ")}`;
        return [
          failed(NAME, GEOMEAN_LABEL, geomeanTarget, reason),
          failed(NAME, WORST_LABEL, worstTarget, reason),
        ];
      }
      const worst = ratios.reduce((a, b) => (b.ratio > a.ratio ? b : a));
      const results: TargetResult[] = [
        judge(
          NAME,
          GEOMEAN_LABEL,
          geomean(ratios.map((entry) => entry.ratio)),
          GEOMEAN_LIMIT,
          "x",
          "at-most",
          ratios.map((entry) => `${entry.name} ${entry.ratio.toFixed(2)}x`).join(", "),
        ),
        judge(NAME, WORST_LABEL, worst.ratio, CASE_LIMIT, "x", "at-most", worst.name),
      ];
      return results;
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      return [
        failed(NAME, GEOMEAN_LABEL, geomeanTarget, reason),
        failed(NAME, WORST_LABEL, worstTarget, reason),
      ];
    }
  },
};
