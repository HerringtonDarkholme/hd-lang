// host-call-overhead: what one call from a program to its host costs.
//
// Three loops (fixtures/host-call-overhead/), each against an empty loop of
// the same count that does the same local work without the call:
// - Console: `println("x")`, 100,000 times;
// - Fs read: `read_text!` of the package's `hd.toml`, 10,000 times;
// - serde boundary: `FsRead.list_dir!` of `src/`, which returns a list of
//   4 `Entry` records, 10,000 times.
// After a warm-up, each loop and its empty loop run RUNS times,
// interleaved; the cost per call is the median of (loop run i - empty run
// i) / count.
// Targets (Pillar 3): Console ≤ 1 µs; Fs read ≤ 10 µs; serde boundary
// ≤ 5 µs. The architecture names "≤ 1 µs for a scalar call"; the Fs and
// serde budgets are this harness's proposal, since a file read makes
// system calls and a record list is copied across.
// n/a: never; a program that fails to build or run fails its line.

import { buildFixture, howNote, timeInterleaved, unitCost } from "../lib/artifact.ts";
import type { ProgramName } from "../lib/fixtures.ts";
import type { TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "host-call-overhead";
const RUNS = 5;
const TIMEOUT_MS = 120_000;

interface Crossing {
  readonly label: string;
  readonly count: number;
  readonly limitUs: number;
  /** The program fixture with the call, and the one with the same local work without it. */
  readonly call: ProgramName;
  readonly empty: ProgramName;
}

const CROSSINGS: readonly Crossing[] = [
  {
    label: "Console println, per call",
    count: 100_000,
    limitUs: 1,
    call: "host-call-overhead/println",
    empty: "host-call-overhead/println-empty",
  },
  {
    label: "Fs read_text!, per call",
    count: 10_000,
    limitUs: 10,
    call: "host-call-overhead/read-text",
    empty: "host-call-overhead/read-text-empty",
  },
  {
    label: "serde boundary (list_dir!), per call",
    count: 10_000,
    limitUs: 5,
    call: "host-call-overhead/list-dir",
    empty: "host-call-overhead/list-dir-empty",
  },
];

export const hostCallOverhead: Metric = {
  name: NAME,
  pillar: 3,
  summary: "cost per host call: Console, Fs read, a serde-boundary call; loop minus empty loop",
  async run(context) {
    const results: TargetResult[] = [];
    context.log(`${NAME}: ${CROSSINGS.length} crossings, ${RUNS} runs each after a warm-up`);
    for (const crossing of CROSSINGS) {
      const target = `≤ ${formatValue(crossing.limitUs, "us")}`;
      try {
        const call = await buildFixture(context.hd, crossing.call, { count: crossing.count });
        const empty = await buildFixture(context.hd, crossing.empty, { count: crossing.count });
        const [callMs, emptyMs] = await timeInterleaved(
          [call, empty],
          RUNS,
          crossing.label,
          TIMEOUT_MS,
        );
        const us = unitCost(callMs!, emptyMs!, crossing.count) * 1000;
        results.push(
          judge(
            NAME,
            crossing.label,
            Math.max(us, 0),
            crossing.limitUs,
            "us",
            "at-most",
            `${crossing.count} calls; ${howNote(call)}`,
          ),
        );
      } catch (error) {
        results.push(failed(NAME, crossing.label, target, errorText(error)));
      }
    }
    return results;
  },
};
