// conformance: the conformance suite passes, except the known failures.
//
// Every indexed spec/conformance case must pass except the rows of the root
// KNOWN_FAILURES.tsv, and the list must reach 0.
// - known failures: the rows of KNOWN_FAILURES.tsv;
// - pass rate (only with --suite): runs the two cargo conformance tiers in
//   the repository root (`hd_driver --test conformance` and
//   `hd_cli --test cli_conformance --features wasmtime`, as CI runs the CLI
//   cases with the wasmtime engine; the `--hd` command is not used) and
//   reads their summary lines: passed / (total - known).
// It takes minutes, so the pass rate runs only with --suite.
// Targets (gate): 100% of the cases not listed; 0 known failures.
// n/a: the pass rate without --suite ("run with --suite").

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { runProcess } from "../lib/hd.ts";
import { failed, judge, notApplicable, type Metric, type TargetResult } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "conformance";
const RATE_LABEL = "pass rate, known failures excluded";
const RATE_TARGET = "≥ 100.0%";
const KNOWN_LABEL = "listed known failures";
const KNOWN_TARGET = "≤ 0";
const TIMEOUT_MS = 30 * 60_000;
/** Each tier's crate, test target and cargo features. */
const TIERS = [
  ["hd_driver", "conformance", []],
  ["hd_cli", "cli_conformance", ["--features", "wasmtime"]],
] as const;

/**
 * The counts of the conformance tiers' summary lines
 * (`driver conformance on node, dev pipeline: pass N, known K, fail F,
 * unsupported U, total T`), summed over every tier in the output.
 */
export function conformanceCounts(
  output: string,
): { readonly passed: number; readonly selected: number } | undefined {
  let passed = 0;
  let selected = 0;
  let found = false;
  for (const line of output.matchAll(
    /^\S+ conformance on [^:\n]+: pass (\d+), known (\d+), fail \d+, unsupported \d+, total (\d+)$/gm,
  )) {
    found = true;
    passed += Number(line[1]);
    selected += Number(line[3]) - Number(line[2]);
  }
  return found ? { passed, selected } : undefined;
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
  summary: "conformance pass rate without the known failures, and the known failures left",
  async run(context) {
    const known = tsvRows(
      readFileSync(join(context.repoRoot, "KNOWN_FAILURES.tsv"), "utf8"),
    ).length;
    const knownLine: TargetResult = {
      ...judge(NAME, KNOWN_LABEL, known, 0, "count", "at-most", "KNOWN_FAILURES.tsv"),
      target: KNOWN_TARGET,
    };
    if (!context.suite) {
      return [
        { ...notApplicable(NAME, RATE_LABEL, RATE_TARGET, ""), note: "run with --suite" },
        knownLine,
      ];
    }
    context.log(`${NAME}: the cargo conformance tiers, up to ${TIMEOUT_MS / 60_000} min`);
    let output = "";
    for (const [crate, test, features] of TIERS) {
      const result = await runProcess(
        ["cargo", "test", "-q", "-p", crate, "--test", test, ...features],
        {
          cwd: context.repoRoot,
          timeoutMs: TIMEOUT_MS,
          env: { HD_CACHE: makeTempDir(`${NAME}-cache`) },
        },
      );
      if (result.timedOut) {
        return [failed(NAME, RATE_LABEL, RATE_TARGET, `${test} timed out`), knownLine];
      }
      output += result.stdout;
    }
    const counts = conformanceCounts(output);
    if (!counts || counts.selected === 0) {
      return [failed(NAME, RATE_LABEL, RATE_TARGET, "no summary line"), knownLine];
    }
    return [
      judge(
        NAME,
        RATE_LABEL,
        counts.passed / counts.selected,
        1,
        "%",
        "at-least",
        `${counts.passed} of ${counts.selected} unlisted cases passed`,
      ),
      knownLine,
    ];
  },
};
