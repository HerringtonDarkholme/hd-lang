// host-call-overhead: what one call from a program to its host costs.
//
// Three loops, each against an empty loop of the same count that does the
// same local work without the call:
// - Console: `println("x")`, 100,000 times;
// - Fs read: `read_text!` of the 25-byte `hd.toml`, 10,000 times;
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

import { buildProgram, howNote, timeInterleaved, unitCost } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "host-call-overhead";
const RUNS = 5;
const TIMEOUT_MS = 120_000;

interface Crossing {
  readonly label: string;
  readonly count: number;
  readonly limitUs: number;
  /** The loop body: a call, or the same local work without it. */
  readonly body: (call: boolean) => readonly string[];
}

const CROSSINGS: readonly Crossing[] = [
  {
    label: "Console println, per call",
    count: 100_000,
    limitUs: 1,
    body: (call) => (call ? ['        println("x")'] : ["        total = total + 1"]),
  },
  {
    label: "Fs read_text!, per call",
    count: 10_000,
    limitUs: 10,
    body: (call) =>
      call
        ? [
            '        text := read_text!(Path("hd.toml")).expect("hd.toml")',
            "        total = total + text.len()",
          ]
        : ['        text := "${i}"', "        total = total + text.len()"],
  },
  {
    label: "serde boundary (list_dir!), per call",
    count: 10_000,
    limitUs: 5,
    body: (call) =>
      call
        ? [
            '        found := $.use(FsRead).list_dir!(Path("src")).expect("src")',
            "        total = total + found.len()",
          ]
        : ["        let found: List[i64] = [i, i, i, i]", "        total = total + found.len()"],
  },
];

/** The program of one crossing: its loop, with or without the call. */
export function crossingProgram(crossing: Crossing, call: boolean): ReadonlyMap<string, string> {
  return new Map([
    ["hd.toml", '[package]\nname = "bench"\n'],
    [
      "src/main.hd",
      [
        "use std.fs.{FsRead, read_text}",
        "use std.path.Path",
        "",
        "pub fn main!() -> void $ Console + FsRead:",
        "    let total: usize = 0",
        "    let i: i64 = 0",
        `    while i < ${crossing.count}:`,
        ...crossing.body(call),
        "        i = i + 1",
        '    println("total ${total}")',
        "",
      ].join("\n"),
    ],
    // Three more files, so list_dir! of src/ returns 4 entries.
    ...["a", "b", "c"].map((name): [string, string] => [
      `src/${name}.hd`,
      `pub fn ${name}() -> i32: 1\n`,
    ]),
  ]);
}

export const hostCallOverhead: Metric = {
  name: NAME,
  pillar: 3,
  summary: "cost per host call: Console, Fs read, a serde-boundary call; loop minus empty loop",
  async run(context) {
    const results: TargetResult[] = [];
    context.log(`${NAME}: ${CROSSINGS.length} crossings, ${RUNS} runs each after a warm-up`);
    for (const [index, crossing] of CROSSINGS.entries()) {
      const target = `≤ ${formatValue(crossing.limitUs, "us")}`;
      try {
        const call = await buildProgram(
          context.hd,
          crossingProgram(crossing, true),
          `host-${index}`,
        );
        const empty = await buildProgram(
          context.hd,
          crossingProgram(crossing, false),
          `host-${index}-empty`,
        );
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
        results.push(
          failed(
            NAME,
            crossing.label,
            target,
            error instanceof Error ? error.message : String(error),
          ),
        );
      }
    }
    return results;
  },
};
