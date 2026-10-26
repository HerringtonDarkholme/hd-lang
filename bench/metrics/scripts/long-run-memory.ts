// long-run-memory: a long-running program's memory stays flat.
//
// A simulated service (fixtures/long-run-memory/service.hd): a loop that
// encodes a JSON request, decodes it, counts the visit in a map of 500
// sessions, and encodes a response. Its state is bounded, so its memory should be too. It prints a progress line
// every 2,000 requests. The program runs for 60 s (10 min with --long);
// the RSS of its process group is sampled with `ps` 30 times. RSS stands
// in for the heap, since no portable heap counter exists.
// The growth is the median of the last 3 samples minus the median of the
// 3 samples after the warm-up, the first quarter of the run.
// Target (Pillar 3): flat after warm-up, here a growth ≤ 10 MB (this
// harness's bound, as in long-session).
// n/a: never; a program that fails to build, exits early, or serves no
// request fails the line.

import { buildFixture, howNote } from "../lib/artifact.ts";
import { runSampled } from "../lib/hd.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";
import { p50 } from "../lib/stats.ts";

const NAME = "long-run-memory";
const SHORT_MS = 60_000;
const LONG_MS = 10 * 60_000;
const SAMPLES = 30;
const LIMIT = 10 * 1024 * 1024;
const TARGET = `≤ ${formatValue(LIMIT, "MB")}`;

/** The growth from the samples after the warm-up quarter to the last ones; NaN when too few. */
export function flatGrowth(samples: readonly (number | undefined)[]): number {
  const known = samples.filter((value): value is number => value !== undefined);
  const start = Math.floor(known.length / 4);
  if (known.length - start < 6) return Number.NaN;
  return p50(known.slice(-3)) - p50(known.slice(start, start + 3));
}

export const longRunMemory: Metric = {
  name: NAME,
  pillar: 3,
  summary: "RSS of a simulated service over 60 s (10 min with --long): flat after warm-up",
  async run(context) {
    const durationMs = context.long ? LONG_MS : SHORT_MS;
    const label = `RSS growth after warm-up, ${durationMs / 60_000} min`;
    try {
      const program = await buildFixture(context.hd, "long-run-memory/service");
      context.log(`${NAME}: the service runs ${durationMs / 1000} s`);
      const run = await runSampled(program.argv, {
        cwd: program.dir,
        durationMs,
        intervalMs: durationMs / SAMPLES,
      });
      const served = [...run.stdout.matchAll(/served=(\d+)/g)].at(-1)?.[1];
      if (run.exited !== undefined)
        return [
          failed(
            NAME,
            label,
            TARGET,
            `the service exited ${String(run.exited)}: ${(run.stdout + run.stderr).trim().slice(-200)}`,
          ),
        ];
      if (served === undefined)
        return [failed(NAME, label, TARGET, "the service reported no request")];
      const rss = run.samples.map((sample) => sample.rssBytes);
      const growth = flatGrowth(rss);
      if (Number.isNaN(growth))
        return [failed(NAME, label, TARGET, "ps reported too few RSS samples")];
      const known = rss.filter((value): value is number => value !== undefined);
      return [
        judge(
          NAME,
          label,
          growth,
          LIMIT,
          "MB",
          "at-most",
          `${served} requests; RSS ${formatValue(known[Math.floor(known.length / 4)]!, "MB")} after warm-up, ${formatValue(known.at(-1)!, "MB")} at the end; ${howNote(program)}`,
        ),
      ];
    } catch (error) {
      return [failed(NAME, label, TARGET, errorText(error))];
    }
  },
};
