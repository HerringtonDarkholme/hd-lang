// size-startup-heap: how big user programs are, how fast they start, and
// how much heap they need, against Node.
//
// - Size: the release Wasm file of a tiny program that prints one line,
//   and, in the note, of each test/perf/micro case.
// - Start-up: the tiny program run 10 times after a warm-up; the time from
//   starting the process to its first byte of output, median. It includes
//   the start-up of whatever runs the module, such as `hd FILE.wasm`.
// - Heap: peak RSS, from /usr/bin/time, of the map and string-build micro
//   cases, minus the peak RSS of a program that does no work, median of 3
//   runs each. Node runs the case's `main` once, minus an empty program.
//   RSS stands in for the heap, since no portable heap counter exists. A
//   Node growth under 1 MB counts as 1 MB, so noise cannot blow up the ratio.
// Targets (Pillar 3): tiny ≤ 2 KB; first output ≤ 5 ms; heap ≤ 2x Node.
// n/a: the size line when the build writes no .wasm file; the heap lines
// with no /usr/bin/time.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { buildProgram, howNote, runProgram, type Program } from "../lib/artifact.ts";
import { firstLines, runProblem } from "../lib/fixture.ts";
import { canMeasure, runProcess } from "../lib/hd.ts";
import type { MetricContext, TargetResult } from "../lib/metric.ts";
import { failed, formatValue, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";
import { makeTempDir } from "../lib/tmp.ts";
import { BASELINE_PROGRAM, MICRO_CASES, microSource } from "./runtime.ts";

const NAME = "size-startup-heap";
const TIMEOUT_MS = 60_000;
const SIZE_LIMIT = 2048;
const FIRST_OUTPUT_LIMIT_MS = 5;
const FIRST_OUTPUT_RUNS = 10;
const HEAP_RUNS = 3;
const HEAP_LIMIT = 2;
const HEAP_FLOOR = 1024 * 1024;
const HEAP_CASES = ["map", "string-build"] as const;

export const TINY_PROGRAM = ["pub fn main() -> void $ Console:", '    println("ready")', ""].join(
  "\n",
);

/** A micro case's `main` run once, for a peak heap. */
export function nodeOnce(source: string): string {
  const tail = source.indexOf("const times = [];");
  if (tail < 0) throw new Error("not a micro case program");
  return `${source.slice(0, tail)}main();\n`;
}

const errorText = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);

async function sizeLine(context: MetricContext, tiny: Program): Promise<TargetResult> {
  const label = "tiny program, release Wasm size";
  const target = `≤ ${formatValue(SIZE_LIMIT, "bytes")}`;
  if (tiny.how !== "wasm" || tiny.bytes === undefined)
    return notApplicable(NAME, label, target, `the build wrote no .wasm file (${howNote(tiny)})`);
  const sizes: string[] = [];
  for (const name of MICRO_CASES) {
    try {
      const program = await buildProgram(context.hd, microSource(context, name), `size-${name}`);
      sizes.push(`${name} ${program.bytes ?? "?"} B`);
    } catch (error) {
      sizes.push(`${name} failed: ${errorText(error).slice(0, 80)}`);
    }
  }
  return judge(
    NAME,
    label,
    tiny.bytes,
    SIZE_LIMIT,
    "bytes",
    "at-most",
    `${tiny.release ? "" : "debug build (no --release); "}benchmarks: ${sizes.join(", ")}`,
  );
}

async function firstOutputLine(tiny: Program): Promise<TargetResult> {
  const label = "tiny program, start to first output";
  const target = `≤ ${formatValue(FIRST_OUTPUT_LIMIT_MS, "ms")}`;
  try {
    const samples: number[] = [];
    for (let run = -1; run < FIRST_OUTPUT_RUNS; run++) {
      const result = await runProgram(tiny, "the tiny program", { timeoutMs: TIMEOUT_MS });
      if (!result.stdout.includes("ready"))
        throw new Error(`the tiny program printed ${JSON.stringify(result.stdout.slice(0, 80))}`);
      if (run >= 0) samples.push(result.firstOutputMs ?? result.wallMs);
    }
    return judge(NAME, label, p50(samples), FIRST_OUTPUT_LIMIT_MS, "ms", "at-most", howNote(tiny));
  } catch (error) {
    return failed(NAME, label, target, errorText(error));
  }
}

/** The median peak RSS of a command over HEAP_RUNS runs. */
async function peakRss(argv: readonly string[], cwd: string, what: string): Promise<number> {
  const samples: number[] = [];
  for (let run = 0; run < HEAP_RUNS; run++) {
    const result = await runProcess(argv, { cwd, timeoutMs: TIMEOUT_MS, measure: true });
    const problem = runProblem(result, TIMEOUT_MS, what);
    if (problem) throw new Error(problem);
    if (result.rssBytes === undefined)
      throw new Error(`no peak RSS for ${what}: ${firstLines(result)}`);
    samples.push(result.rssBytes);
  }
  return p50(samples);
}

async function heapLines(context: MetricContext): Promise<TargetResult[]> {
  const target = `≤ ${HEAP_LIMIT.toFixed(2)}x`;
  const labels = HEAP_CASES.map((name) => `peak heap, ${name}: hd / Node`);
  if (!canMeasure())
    return labels.map((label) =>
      notApplicable(NAME, label, target, "no /usr/bin/time on this host"),
    );
  let base: { readonly hd: number; readonly node: number };
  const nodeDir = makeTempDir("node-heap");
  try {
    const baseline = await buildProgram(context.hd, BASELINE_PROGRAM, "heap-baseline");
    writeFileSync(join(nodeDir, "empty.mjs"), "\n");
    base = {
      hd: await peakRss(baseline.argv, baseline.dir, "the baseline program"),
      node: await peakRss(
        [process.execPath, join(nodeDir, "empty.mjs")],
        nodeDir,
        "node empty.mjs",
      ),
    };
  } catch (error) {
    return labels.map((label) => failed(NAME, label, target, errorText(error)));
  }
  const results: TargetResult[] = [];
  for (const [index, name] of HEAP_CASES.entries()) {
    const label = labels[index]!;
    try {
      const program = await buildProgram(context.hd, microSource(context, name), `heap-${name}`);
      const hd = (await peakRss(program.argv, program.dir, name)) - base.hd;
      const file = join(nodeDir, `${name}.mjs`);
      const source = readFileSync(
        join(context.repoRoot, "test", "perf", "micro", `${name}.js`),
        "utf8",
      );
      writeFileSync(file, nodeOnce(source));
      const node =
        (await peakRss([process.execPath, file], nodeDir, `node ${name}.mjs`)) - base.node;
      results.push(
        judge(
          NAME,
          label,
          hd / Math.max(node, HEAP_FLOOR),
          HEAP_LIMIT,
          "x",
          "at-most",
          `hd +${formatValue(hd, "MB")}, Node +${formatValue(node, "MB")} over an empty program`,
        ),
      );
    } catch (error) {
      results.push(failed(NAME, label, target, errorText(error)));
    }
  }
  return results;
}

export const sizeStartupHeap: Metric = {
  name: NAME,
  pillar: 3,
  summary: "release Wasm size, start to first output, and peak heap against Node",
  async run(context) {
    let tiny: Program;
    try {
      tiny = await buildProgram(context.hd, TINY_PROGRAM, "tiny");
    } catch (error) {
      return [
        failed(
          NAME,
          "tiny program, release Wasm size",
          `≤ ${formatValue(SIZE_LIMIT, "bytes")}`,
          errorText(error),
        ),
      ];
    }
    context.log(
      `${NAME}: sizes, ${FIRST_OUTPUT_RUNS} start-ups, peak heap of ${HEAP_CASES.join(" and ")}`,
    );
    return [
      await sizeLine(context, tiny),
      await firstOutputLine(tiny),
      ...(await heapLines(context)),
    ];
  },
};
