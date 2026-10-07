// suite-cpu: total CPU time of the conformance suite.
//
// Runs `test/run-portable.ts --suite conformance --compiler HD`, which runs
// every spec/conformance case against the hd under test as a child
// process, and measures it with /usr/bin/time: the CPU time counts the
// runner and every process it waited for. Its pass or fail is not judged
// here; the note gives its exit status.
// It takes minutes, so it runs only with --suite.
// Target (Pillar 2): ≤ 60 s.
// n/a: without --suite ("run with --suite"), or with no /usr/bin/time.

import { join } from "node:path";

import { firstLines } from "../lib/fixture.ts";
import { canMeasure, runProcess } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric } from "../lib/metric.ts";

const NAME = "suite-cpu";
const LABEL = "conformance suite, total CPU";
const TARGET = "≤ 60.0 s";
const LIMIT_S = 60;
const TIMEOUT_MS = 30 * 60_000;

/** Quotes a word for the runner's --compiler value, which splits on spaces outside quotes. */
const quote = (word: string): string => (/[\s"']/.test(word) ? `"${word}"` : word);

export const suiteCpu: Metric = {
  name: NAME,
  pillar: 2,
  summary: "total CPU of the conformance suite (only with --suite)",
  async run(context) {
    if (!context.suite)
      return [{ ...notApplicable(NAME, LABEL, TARGET, ""), note: "run with --suite" }];
    if (!canMeasure()) return [notApplicable(NAME, LABEL, TARGET, "no /usr/bin/time on this host")];
    context.log(`${NAME}: the conformance suite, up to ${TIMEOUT_MS / 60_000} min`);
    const runner = join(context.repoRoot, "test", "run-portable.ts");
    const compiler = context.hd.argv.map(quote).join(" ");
    const result = await runProcess(
      [
        process.execPath,
        "--experimental-strip-types",
        runner,
        "--suite",
        "conformance",
        "--compiler",
        compiler,
      ],
      { cwd: context.repoRoot, timeoutMs: TIMEOUT_MS, measure: true },
    );
    if (result.timedOut)
      return [failed(NAME, LABEL, TARGET, `timed out after ${TIMEOUT_MS / 60_000} min`)];
    if (result.cpuMs === undefined)
      return [failed(NAME, LABEL, TARGET, `no CPU time measured: ${firstLines(result)}`)];
    const status =
      result.status === 0 ? "the suite passed" : `the suite exited ${String(result.status)}`;
    return [judge(NAME, LABEL, result.cpuMs / 1000, LIMIT_S, "s", "at-most", status)];
  },
};
