// runtime: how fast user programs run, against Node.
//
// The cases are the microbenchmarks of test/perf/micro: recursive fib(30),
// the sum of 1..10M, building a string of 100k parts, a map with 100k
// inserts and lookups, and sorting 100k items. Each checks its own result.
// hd: each case is built as an executable (lib/artifact.ts), next to a
// baseline program that does no work. After one warm-up run of each, they
// run RUNS times, interleaved, and a case's time is run i minus baseline
// run i, which cancels start-up and instantiation. The line gives the
// median and the 10th and 90th percentiles.
// Node: the case's .js `main`, wrapped in a self-timing program that runs
// it 3 times to warm up, then times 7 runs; the median counts. Python's
// median, from the case's .py, is in the note when python3 is installed.
// Targets (Pillar 3): geomean of hd/Node ≤ 1.5x; every case ≤ 3x.
// n/a: never; a case that fails to build or run fails its line.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  buildProgram,
  howNote,
  nodeMedian,
  runNodeProgram,
  timeInterleaved,
  type Program,
} from "../lib/artifact.ts";
import { runProcess } from "../lib/hd.ts";
import type { MetricContext, TargetResult } from "../lib/metric.ts";
import { failed, judge, type Metric } from "../lib/metric.ts";
import { geomean, spread } from "../lib/stats.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "runtime";
const RUNS = 7;
const TIMEOUT_MS = 60_000;
const GEOMEAN_LIMIT = 1.5;
const CASE_LIMIT = 3;

/** The cases of test/perf/micro, as their JavaScript and Python files name them. */
export const MICRO_CASES = ["fib", "sum", "string-build", "map", "sort"] as const;

/** A program with no work, to subtract start-up from each case. */
export const BASELINE_PROGRAM = [
  "pub fn main() -> void:",
  '    if 1 + 1 != 2: panic("wrong")',
  "",
].join("\n");

const microDir = (context: MetricContext): string =>
  join(context.repoRoot, "test", "perf", "micro");

/** The hd source of a micro case; hd module paths spell `-` as `_`. */
export const microSource = (context: MetricContext, name: string): string =>
  readFileSync(join(microDir(context), `${name.replaceAll("-", "_")}.hd`), "utf8");

/**
 * The self-timing Node body for a micro case: the case's own `main`, with
 * its timing tail replaced by `measure`.
 */
export function nodeBody(source: string): string {
  const tail = source.indexOf("const times = [];");
  if (tail < 0 || !source.includes("function main(")) throw new Error("not a micro case program");
  return `${source.slice(0, tail)}console.log(JSON.stringify({ ms: measure(main) }));\n`;
}

/** The median ms that a micro case's Python program prints, or undefined without python3. */
async function pythonMs(context: MetricContext, name: string): Promise<number | undefined> {
  try {
    const result = await runProcess(["python3", join(microDir(context), `${name}.py`)], {
      cwd: makeTempDir("python"),
      timeoutMs: TIMEOUT_MS,
    });
    const value = Number(result.stdout.trim());
    return result.status === 0 && Number.isFinite(value) ? value : undefined;
  } catch {
    return undefined;
  }
}

const fmt = (ms: number): string => (ms < 10 ? ms.toFixed(1) : String(Math.round(ms)));

export const runtime: Metric = {
  name: NAME,
  pillar: 3,
  summary: "microbenchmarks of user programs: median and spread, against Node",
  async run(context) {
    const results: TargetResult[] = [];
    const ratios: number[] = [];
    let baseline: Program;
    try {
      baseline = await buildProgram(context.hd, BASELINE_PROGRAM, "runtime-baseline");
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      return [failed(NAME, "geomean hd / Node", `≤ ${GEOMEAN_LIMIT.toFixed(2)}x`, reason)];
    }
    context.log(`${NAME}: ${MICRO_CASES.length} cases, ${RUNS} runs each after a warm-up`);
    for (const name of MICRO_CASES) {
      const label = `${name}: hd / Node`;
      const target = `≤ ${CASE_LIMIT.toFixed(2)}x`;
      try {
        const program = await buildProgram(
          context.hd,
          microSource(context, name),
          `runtime-${name}`,
        );
        const [caseMs, baseMs] = await timeInterleaved(
          [program, baseline],
          RUNS,
          `${name}`,
          TIMEOUT_MS,
        );
        const work = spread(caseMs!.map((ms, index) => ms - baseMs![index]!));
        const source = readFileSync(join(microDir(context), `${name}.js`), "utf8");
        const node = nodeMedian(await runNodeProgram(nodeBody(source), name), "ms");
        const python = await pythonMs(context, name);
        const note =
          `hd ${fmt(work.p50)} ms (p10 ${fmt(work.p10)}, p90 ${fmt(work.p90)}), Node ${fmt(node)} ms` +
          `${python === undefined ? "" : `, Python ${fmt(python)} ms`}; ${howNote(program)}`;
        if (work.p50 <= 0) {
          results.push(failed(NAME, label, target, `no work measured beyond start-up; ${note}`));
          continue;
        }
        const ratio = work.p50 / Math.max(node, 0.01);
        ratios.push(ratio);
        results.push(judge(NAME, label, ratio, CASE_LIMIT, "x", "at-most", note));
      } catch (error) {
        results.push(
          failed(NAME, label, target, error instanceof Error ? error.message : String(error)),
        );
      }
    }
    const allMeasured = ratios.length === MICRO_CASES.length;
    results.unshift(
      allMeasured
        ? judge(
            NAME,
            "geomean hd / Node",
            geomean(ratios),
            GEOMEAN_LIMIT,
            "x",
            "at-most",
            `${ratios.length} cases`,
          )
        : failed(
            NAME,
            "geomean hd / Node",
            `≤ ${GEOMEAN_LIMIT.toFixed(2)}x`,
            `${MICRO_CASES.length - ratios.length} of ${MICRO_CASES.length} cases not measured`,
          ),
    );
    return results;
  },
};
