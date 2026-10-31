// dev-speed: how slow code from the dev pipeline may run, with the profile
// fixed.
//
// The cases are the `runtime` micro cases (bench/micro), each turned
// into a test case (fixtures/dev-speed/case-test.hd.tmpl): the case's
// `main` becomes a plain function that one test runs REPEAT times, so the
// optimized pipeline's work stays above the noise. `hd test` may use the dev pipeline, and `hd test
// --release` uses the optimized one; both keep the test profile
// (cli.profile.test, cli.profile.test.release, cli.profile.pipeline.release).
// A case's time in a pipeline is its median wall time minus the median of
// an empty test, which cancels build and start-up time. Optimized work
// under 50 ms is within noise and fails.
// Targets (Pillar 1): dev ≤ 4x optimized, geomean; no case over 10x.
// n/a: when `hd help test` names no `--release` flag, as with an
// implementation that has no separate pipelines.

import { MICRO_CASES, microSource } from "../lib/artifact.ts";
import {
  manifest,
  renderFixture,
  writePackage,
  writeProgram,
  type Files,
} from "../lib/fixtures.ts";
import { supportsFlag, timedSeries } from "../lib/hd.ts";
import {
  errorText,
  failed,
  judge,
  notApplicable,
  type Metric,
  type MetricContext,
  type TargetResult,
} from "../lib/metric.ts";
import { geomean, p50 } from "../lib/stats.ts";

const NAME = "dev-speed";
const RUNS = 3;
const NOISE_MS = 50;
const TIMEOUT_MS = 120_000;
const GEOMEAN_LIMIT = 4;
const CASE_LIMIT = 10;
const GEOMEAN_LABEL = "geomean dev / optimized";
const WORST_LABEL = "worst case dev / optimized";
/** How many times a test runs its case. */
const REPEAT = 30;

/** A micro case as test source: its `main` becomes `case_main`, which one test case runs. */
export function asTestCase(source: string, repeat = REPEAT): string {
  if (!source.includes("pub fn main() -> void:")) {
    throw new Error("not a micro case program");
  }
  return renderFixture("dev-speed/case-test.hd.tmpl", {
    case: source.replace("pub fn main() -> void:", "fn case_main() -> void:").trimEnd(),
    repeat,
  });
}

/** The package of a micro case as a test. */
export const casePackage = (context: MetricContext, name: string): Files =>
  new Map([
    ["hd.toml", manifest("case")],
    ["src/main.hd", asTestCase(microSource(context, name))],
  ]);

export const devSpeed: Metric = {
  name: NAME,
  pillar: 1,
  summary: "run time of the micro cases as tests, dev pipeline vs optimized pipeline",
  async run(context) {
    const empty = writeProgram("dev-speed/empty-test", {}, "dev-empty");
    const geomeanTarget = `≤ ${GEOMEAN_LIMIT.toFixed(2)}x`;
    const worstTarget = `≤ ${CASE_LIMIT.toFixed(2)}x`;
    if (!(await supportsFlag(context.hd, "test", "--release", empty))) {
      return [
        notApplicable(NAME, GEOMEAN_LABEL, geomeanTarget, "hd test has no --release"),
        notApplicable(NAME, WORST_LABEL, worstTarget, "hd test has no --release"),
      ];
    }
    const time = async (dir: string, optimized: boolean) => {
      const args = optimized ? ["test", "--release"] : ["test"];
      const series = await timedSeries(
        context.hd,
        RUNS,
        () => args,
        { cwd: dir, timeoutMs: TIMEOUT_MS },
        `hd ${args.join(" ")}`,
      );
      if (series.problem) {
        throw new Error(series.problem);
      }
      return p50(series.samples);
    };
    try {
      context.log(`${NAME}: ${MICRO_CASES.length} cases, ${RUNS} runs each in 2 pipelines`);
      const emptyDev = await time(empty, false);
      const emptyOptimized = await time(empty, true);
      const ratios: { name: string; ratio: number }[] = [];
      const noisy: string[] = [];
      for (const name of MICRO_CASES) {
        const dir = writePackage(`dev-${name}`, casePackage(context, name));
        const dev = (await time(dir, false)) - emptyDev;
        const optimized = (await time(dir, true)) - emptyOptimized;
        if (optimized < NOISE_MS) {
          noisy.push(`${name} (${optimized.toFixed(1)} ms)`);
        } else {
          ratios.push({ name, ratio: dev / optimized });
        }
      }
      if (noisy.length > 0) {
        const reason = `optimized work is within noise: ${noisy.join(", ")}`;
        return [
          failed(NAME, GEOMEAN_LABEL, geomeanTarget, reason),
          failed(NAME, WORST_LABEL, worstTarget, reason),
        ];
      }
      const worst = ratios.reduce((accumulator, value) =>
        value.ratio > accumulator.ratio ? value : accumulator,
      );
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
      return [
        failed(NAME, GEOMEAN_LABEL, geomeanTarget, errorText(error)),
        failed(NAME, WORST_LABEL, worstTarget, errorText(error)),
      ];
    }
  },
};
