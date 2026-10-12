// suspension-overhead: what suspension costs when nothing waits.
//
// `echo!` is a suspending function that returns its argument at once, so
// every await finds a ready value. Each loop runs COUNT times and is timed
// against an empty loop that calls the plain `echo_plain` instead:
// - await: `total = total + echo!(i)`;
// - all!: `(a, b) := all!(echo(i), echo(i + 1))`, two tasks per iteration;
// - race!: `race!(echo(i), echo(i))`, two tasks per iteration.
// After a warm-up, each loop and the empty loop run RUNS times,
// interleaved; the cost per iteration is the median of (loop run i - empty
// run i) / COUNT. Throughput is tasks per second: 2 / cost.
// Targets (Pillar 3), this harness's proposal, since the architecture
// names a budget without a number: ≤ 100 ns per ready await; ≥ 1M tasks/s
// for all! and race!.
// n/a: never; a program that fails to build or run fails its line.

import { buildProgram, howNote, timeInterleaved, unitCost } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "suspension-overhead";
const COUNT = 200_000;
const RUNS = 5;
const TIMEOUT_MS = 120_000;
const AWAIT_LIMIT_NS = 100;
const TASKS_LIMIT = 1_000_000;

/** A loop of COUNT iterations whose body adds to `total`. */
export function suspensionProgram(body: readonly string[]): string {
  return [
    "use std.task.{all, race}",
    "",
    "fn echo!(value: i64) -> i64: value",
    "",
    "fn echo_plain(value: i64) -> i64: value",
    "",
    "fn work!() -> i64:",
    "    let total: i64 = 0",
    "    let i: i64 = 0",
    `    while i < ${COUNT}:`,
    ...body,
    "        i = i + 1",
    "    total",
    "",
    "pub fn main!() -> void $ Console:",
    '    println("${work!()}")',
    "",
  ].join("\n");
}

const EMPTY = ["        total = total + echo_plain(i) + echo_plain(i + 1)"];

const LOOPS = [
  {
    label: "await of a ready value, per await",
    body: ["        total = total + echo!(i) + echo_plain(i + 1)"],
    tasks: false,
  },
  {
    label: "all! of two ready tasks, throughput",
    body: ["        let (a, b) = all!(echo(i), echo(i + 1))", "        total = total + a + b"],
    tasks: true,
  },
  {
    label: "race! of two ready tasks, throughput",
    body: ["        total = total + race!(echo(i), echo(i)) + echo_plain(i + 1)"],
    tasks: true,
  },
] as const;

export const suspensionOverhead: Metric = {
  name: NAME,
  pillar: 3,
  summary: "cost per await of a ready value, and all!/race! task throughput",
  async run(context) {
    const results: TargetResult[] = [];
    const targetOf = (tasks: boolean) =>
      tasks ? `≥ ${formatValue(TASKS_LIMIT, "per-s")}` : `≤ ${formatValue(AWAIT_LIMIT_NS, "ns")}`;
    let empty;
    try {
      empty = await buildProgram(context.hd, suspensionProgram(EMPTY), "suspend-empty");
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      return LOOPS.map((loop) => failed(NAME, loop.label, targetOf(loop.tasks), reason));
    }
    context.log(`${NAME}: ${LOOPS.length} loops of ${COUNT}, ${RUNS} runs each after a warm-up`);
    for (const [index, loop] of LOOPS.entries()) {
      try {
        const program = await buildProgram(
          context.hd,
          suspensionProgram(loop.body),
          `suspend-${index}`,
        );
        const [loopMs, emptyMs] = await timeInterleaved(
          [program, empty],
          RUNS,
          loop.label,
          TIMEOUT_MS,
        );
        const ns = unitCost(loopMs!, emptyMs!, COUNT) * 1e6;
        const note = `${formatValue(ns, "ns")} per iteration; ${howNote(program)}`;
        results.push(
          loop.tasks
            ? // A cost at or below zero is within noise: 0.01 ns keeps the rate finite.
              judge(
                NAME,
                loop.label,
                2e9 / Math.max(ns, 0.01),
                TASKS_LIMIT,
                "per-s",
                "at-least",
                note,
              )
            : judge(NAME, loop.label, Math.max(ns, 0), AWAIT_LIMIT_NS, "ns", "at-most", note),
        );
      } catch (error) {
        results.push(
          failed(
            NAME,
            loop.label,
            targetOf(loop.tasks),
            error instanceof Error ? error.message : String(error),
          ),
        );
      }
    }
    return results;
  },
};
