// conformance: the portable conformance suite passes against this hd.
//
// Runs `test/run-portable.ts --suite conformance --compiler HD`, which runs
// the selected spec/conformance cases (test/portable/cases.tsv) against the
// hd under test as a child process; the cases listed in
// test/portable/KNOWN_FAILURES.tsv are not selected. A case that fails
// only by the time limit reruns once, serially, as the runner does.
// - pass rate: passed / selected, from the runner's summary line;
// - known failures: the rows of KNOWN_FAILURES.tsv, which must reach 0.
// It takes minutes, so it runs only with --suite.
// Targets (gate): 100% of the selected cases; 0 known failures.
// n/a: without --suite ("run with --suite").

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { firstLines } from "../lib/fixture.ts";
import { runProcess } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";

const NAME = "conformance";
const RATE_LABEL = "pass rate, known failures excluded";
const RATE_TARGET = "≥ 100.0%";
const KNOWN_LABEL = "listed known failures";
const KNOWN_TARGET = "≤ 0";
const TIMEOUT_MS = 30 * 60_000;

/** Quotes a word for the runner's --compiler value, which splits on spaces outside quotes. */
const quote = (word: string): string => (/[\s"']/.test(word) ? `"${word}"` : word);

/**
 * The counts of a run-portable conformance run: the summary line's passed
 * and selected, plus the cases a serial retry passed.
 */
export function conformanceCounts(
  output: string,
): { readonly passed: number; readonly selected: number } | undefined {
  const summary = /^conformance: (\d+) passed, (\d+) failed, (\d+) selected/m.exec(output);
  if (!summary) return undefined;
  const retried = /^(\d+) passed after a serial retry/m.exec(output);
  return {
    passed: Number(summary[1]) + Number(retried?.[1] ?? 0),
    selected: Number(summary[3]),
  };
}

/** The data rows of a TSV file with a header row. */
export const tsvRows = (text: string): string[] =>
  text
    .split("\n")
    .slice(1)
    .filter((line) => line.trim() !== "");

export const conformance: Metric = {
  name: NAME,
  pillar: "gate",
  summary: "portable conformance suite pass rate, and the known failures left (only with --suite)",
  async run(context) {
    const known = tsvRows(
      readFileSync(join(context.repoRoot, "test", "portable", "KNOWN_FAILURES.tsv"), "utf8"),
    ).length;
    const knownLine: TargetResult = judge(
      NAME,
      KNOWN_LABEL,
      known,
      0,
      "count",
      "at-most",
      "test/portable/KNOWN_FAILURES.tsv",
    );
    if (!context.suite)
      return [
        { ...notApplicable(NAME, RATE_LABEL, RATE_TARGET, ""), note: "run with --suite" },
        { ...knownLine, target: KNOWN_TARGET },
      ];
    context.log(`${NAME}: the conformance suite, up to ${TIMEOUT_MS / 60_000} min`);
    const runner = join(context.repoRoot, "test", "run-portable.ts");
    const result = await runProcess(
      [
        process.execPath,
        "--experimental-strip-types",
        runner,
        "--suite",
        "conformance",
        "--compiler",
        context.hd.argv.map(quote).join(" "),
      ],
      { cwd: context.repoRoot, timeoutMs: TIMEOUT_MS },
    );
    if (result.timedOut)
      return [
        failed(NAME, RATE_LABEL, RATE_TARGET, `timed out after ${TIMEOUT_MS / 60_000} min`),
        { ...knownLine, target: KNOWN_TARGET },
      ];
    const counts = conformanceCounts(result.stdout);
    if (!counts || counts.selected === 0)
      return [
        failed(NAME, RATE_LABEL, RATE_TARGET, `no summary line: ${firstLines(result)}`),
        { ...knownLine, target: KNOWN_TARGET },
      ];
    return [
      judge(
        NAME,
        RATE_LABEL,
        counts.passed / counts.selected,
        1,
        "%",
        "at-least",
        `${counts.passed} of ${counts.selected} selected cases passed`,
      ),
      { ...knownLine, target: KNOWN_TARGET },
    ];
  },
};
