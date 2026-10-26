// suspension-overhead: what suspension costs when nothing waits.
//
// `echo!` is a suspending function that returns its argument at once, so
// every await finds a ready value. Each loop (fixtures/suspension-overhead/)
// runs COUNT times and is timed against an empty loop that calls the plain
// `echo_plain` instead:
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

import { buildFixture, howNote, timeInterleaved, unitCost } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "suspension-overhead";
const COUNT = 200_000;
const RUNS = 5;
const TIMEOUT_MS = 120_000;
const AWAIT_LIMIT_NS = 100;
const TASKS_LIMIT = 1_000_000;

const EMPTY = "suspension-overhead/empty";

const LOOPS = [
  {
    label: "await of a ready value, per await",
    program: "suspension-overhead/await",
    tasks: false,
  },
  {
    label: "all! of two ready tasks, throughput",
    program: "suspension-overhead/all",
    tasks: true,
  },
  {
    label: "race! of two ready tasks, throughput",
    program: "suspension-overhead/race",
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
      empty = await buildFixture(context.hd, EMPTY, { count: COUNT });
    } catch (error) {
      return LOOPS.map((loop) => failed(NAME, loop.label, targetOf(loop.tasks), errorText(error)));
    }
    context.log(`${NAME}: ${LOOPS.length} loops of ${COUNT}, ${RUNS} runs each after a warm-up`);
    for (const loop of LOOPS) {
      try {
        const program = await buildFixture(context.hd, loop.program, { count: COUNT });
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
        results.push(failed(NAME, loop.label, targetOf(loop.tasks), errorText(error)));
      }
    }
    return results;
  },
};
