// allocations: heap allocation per iteration of user loops.
//
// Four loops of i64 work (fixtures/allocations/): a counted `for` over a
// range, `for` loops over a small list and a small map, n times, and an
// iterator chain, `(0..n).iter().map(...).filter(...).sum()`. Each is built at two
// sizes, SMALL and LARGE iterations, and run 3 times each. The allocated
// bytes per iteration are the median of (bytes at LARGE - bytes at SMALL)
// / (LARGE - SMALL), which cancels start-up.
// This probe measures the Node host heap, not the RC linear heap: a
// module preloaded through NODE_OPTIONS turns on `--trace-gc-nvp` and
// writes `v8.getHeapStatistics()` at exit. The bytes a run allocated are
// the heap in use at exit plus what every collection freed. This works when
// the hd under test runs programs on Node; it does not count RC allocations.
// A count needs an object size, so the bounds are in bytes: below 1 B per
// iteration is no per-iteration allocation, and 32 B holds one small object
// (this harness's bounds).
// Targets (Pillar 3): counted loops, list and map loops among them, 0
// allocations (< 1 B/iter); chain ≤ 1 allocation (≤ 32 B/iter).
// n/a: when the program does not run on Node, so the V8 probe writes
// nothing.

import { buildFixture, heapProbe, howNote, runProgram, type Program } from "../lib/artifact.ts";
import type { ProgramName } from "../lib/fixtures.ts";
import type { TargetResult } from "../lib/metric.ts";
import { errorText, failed, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "allocations";
const SMALL = 1_000;
const LARGE = 1_000_000;
const RUNS = 3;
const TIMEOUT_MS = 60_000;

interface Loop {
  readonly label: string;
  readonly limit: number;
  readonly target: string;
  /** The program fixture, whose param `n` is the iteration count. */
  readonly program: ProgramName;
}

const LOOPS: readonly Loop[] = [
  {
    label: "counted loop, bytes per iteration",
    limit: 0.99,
    target: "0 allocations (< 1 B/iter)",
    program: "allocations/counted",
  },
  {
    label: "list loop, bytes per iteration",
    limit: 0.99,
    target: "0 allocations (< 1 B/iter)",
    program: "allocations/list",
  },
  {
    label: "map loop, bytes per iteration",
    limit: 0.99,
    target: "0 allocations (< 1 B/iter)",
    program: "allocations/map",
  },
  {
    label: "iterator chain, bytes per iteration",
    limit: 32,
    target: "≤ 1 allocation (≤ 32 B/iter)",
    program: "allocations/chain",
  },
];

export const allocations: Metric = {
  name: NAME,
  pillar: 3,
  summary:
    "bytes allocated per iteration of counted, list and map loops and an iterator chain (V8 heap stats)",
  async run(context) {
    const probe = heapProbe();
    const results: TargetResult[] = [];
    context.log(
      `${NAME}: ${LOOPS.length} loops at ${SMALL} and ${LARGE} iterations, ${RUNS} runs each`,
    );
    for (const [index, loop] of LOOPS.entries()) {
      try {
        const build = (n: number, size: string) =>
          buildFixture(context.hd, loop.program, { n }, `alloc-${index}-${size}`);
        const small = await build(SMALL, "small");
        const large = await build(LARGE, "large");
        const bytes = async (program: Program, run: number): Promise<number | undefined> => {
          const tag = `loop${index}-${program === small ? "small" : "large"}-${run}`;
          const result = await runProgram(program, loop.label, {
            timeoutMs: TIMEOUT_MS,
            env: probe.env(tag),
          });
          return probe.allocated(tag, result.stdout);
        };
        const perIteration: number[] = [];
        let missing = false;
        for (let run = 0; run < RUNS; run++) {
          const before = await bytes(small, run);
          const after = await bytes(large, run);
          if (before === undefined || after === undefined) {
            missing = true;
            break;
          }
          perIteration.push((after - before) / (LARGE - SMALL));
        }
        if (missing) {
          results.push(
            notApplicable(
              NAME,
              loop.label,
              loop.target,
              "the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known",
            ),
          );
          continue;
        }
        const value = Math.max(p50(perIteration), 0);
        const judged = judge(
          NAME,
          loop.label,
          value,
          loop.limit,
          "B/iter",
          "at-most",
          howNote(large),
        );
        // The target reads in allocations; the limit behind it is in bytes.
        results.push({ ...judged, target: loop.target });
      } catch (error) {
        results.push(failed(NAME, loop.label, loop.target, errorText(error)));
      }
    }
    return results;
  },
};
