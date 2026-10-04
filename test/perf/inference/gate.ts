// The CI performance gate for the type checker (`pnpm run perf:check`).
//
// Usage: node --experimental-strip-types test/perf/inference/gate.ts
//   [--case NAME]... [--update] [--baseline FILE] [--json FILE]
//
// Each case runs in its own process at a smaller and a larger scale, with
// the reference program (gen.ts REFERENCE) timed in the same rounds. A case
// fails when:
// - it crashes, or runs over the timeout;
// - its time grows by more than 12x per 10x of input (12^log10(step) for
//   another step) and the larger time is over 250 ms; a known super-linear
//   case fails when it grows more than 1.5x its recorded ratio instead;
// - its score (time over the reference program's time) is more than 1.5x its
//   score in baseline.json, and its time is over 100 ms.
// A diagnostic result that differs from baseline.json is a warning: the
// case may no longer measure the path it was written for.
//
// --update records the current scores, ratios, and results in baseline.json.
// README.md explains the rules.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { CASES } from "./gen.ts";
import { formatMs, measureInChild } from "./spawn.ts";

interface GateCase {
  readonly name: string;
  /** The smaller and the larger scale (gen.ts sizes). */
  readonly scales: readonly [number, number];
  /** How the case grows on main, when that breaks or nears the 12x rule. */
  readonly known?: string;
}

// The larger scale takes 0.3 s to 1 s on a laptop. The smaller one takes
// 30 ms or more, so the growth ratio is not noise; for a case that grows
// fast, that puts it at a third of the larger scale, not a tenth.
export const GATE: readonly GateCase[] = [
  { name: "list-nest", scales: [300, 1000], known: "about quadratic in nesting depth" },
  { name: "open-lets-reverse", scales: [5, 50], known: "10x to 16x per 10x" },
  { name: "method-calls-one-var", scales: [30, 300], known: "11x to 13x per 10x" },
  { name: "add-many-types", scales: [10, 30], known: "about n^1.8 in impl types" },
  { name: "overload-candidates", scales: [10, 100] },
  { name: "big-body", scales: [10, 100], known: "11x to 12x per 10x" },
  { name: "mutual-bounds", scales: [10, 30], known: "about cubic in bound pairs" },
  { name: "closures-shared-var", scales: [100, 300], known: "about n^1.6 in closures" },
  { name: "polymorphic-recursion", scales: [30, 300] },
  { name: "late-conflict", scales: [10, 100], known: "10x to 14x per 10x" },
  { name: "occurs-check", scales: [40, 400], known: "11x to 13x per 10x" },
];

const MAX_RATIO_PER_10X = 12;
const KNOWN_RATIO_TOLERANCE = 1.5;
const GROWTH_FLOOR_MS = 250;
const MAX_SLOWDOWN = 1.5;
const SLOWDOWN_FLOOR_MS = 100;
/** In reference units: a smaller-scale score below this is read as this. */
const LOW_SCORE_FLOOR = 0.1;
const REFERENCE_SCALE = 10;
const TIMEOUT_SECONDS = 90;

interface BaselineCase {
  readonly scales: readonly [number, number];
  readonly scores: readonly [number, number];
  readonly ratio: number;
  readonly codes: readonly string[];
}
interface Baseline {
  readonly note: string;
  readonly recorded: { readonly date: string; readonly node: string; readonly platform: string };
  readonly referenceMs: number;
  readonly cases: Record<string, BaselineCase>;
}

const args = process.argv.slice(2);
const selected: string[] = [];
let update = false;
let baselinePath = resolve(import.meta.dirname, "baseline.json");
let jsonPath: string | undefined;
for (let index = 0; index < args.length; index += 1) {
  const option = args[index];
  if (option === "--case") selected.push(args[++index]!);
  else if (option === "--update") update = true;
  else if (option === "--baseline") baselinePath = resolve(args[++index]!);
  else if (option === "--json") jsonPath = args[++index];
  else {
    console.error(`unknown option ${option}`);
    process.exit(2);
  }
}
for (const name of selected)
  if (!GATE.some((entry) => entry.name === name)) {
    console.error(`unknown gate case ${name}`);
    process.exit(2);
  }
for (const entry of GATE)
  if (!CASES.some((perfCase) => perfCase.name === entry.name)) {
    console.error(`gate case ${entry.name} is not in gen.ts`);
    process.exit(2);
  }

let baseline: Baseline | undefined;
try {
  baseline = JSON.parse(readFileSync(baselinePath, "utf8")) as Baseline;
} catch {
  if (!update) {
    console.error(`no baseline at ${baselinePath}; record one with --update`);
    process.exit(2);
  }
}

const formatStep = (step: number): string => String(Number(step.toFixed(2)));

const annotate = (level: "error" | "warning", message: string): void => {
  if (process.env.GITHUB_ACTIONS === "true") console.log(`::${level} title=perf gate::${message}`);
};

const started = performance.now();
const recorded: Record<string, BaselineCase> = { ...baseline?.cases };
const referenceTimes: number[] = [];
const report: Record<string, unknown>[] = [];
let failures = 0;
console.log(
  "| case | scales | small ms | large ms | parse ms | check ms | growth (limit) | vs baseline | verdict |",
);
console.log("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
for (const entry of GATE.filter(
  (gateCase) => selected.length === 0 || selected.includes(gateCase.name),
)) {
  const [small, large] = entry.scales;
  const outcome = measureInChild(
    [entry.name, String(small), String(large), "--reference", String(REFERENCE_SCALE)],
    TIMEOUT_SECONDS,
  );
  const problems: string[] = [];
  const warnings: string[] = [];
  if (outcome.status !== "ok") {
    problems.push(
      outcome.status === "timeout"
        ? `over ${TIMEOUT_SECONDS} s`
        : `crash${outcome.detail ? ` (${outcome.detail})` : ""}`,
    );
    failures += 1;
    console.log(
      `| ${entry.name} | ${small}, ${large} | | | | | | | FAIL: ${problems.join("; ")} |`,
    );
    annotate("error", `${entry.name}: ${problems.join("; ")}`);
    report.push({ case: entry.name, problems });
    continue;
  }
  const { reference, scales } = outcome.report;
  const referenceMs = Math.max(reference!.total, 1);
  referenceTimes.push(referenceMs);
  const [low, high] = [scales[0]!, scales[1]!];
  const scores = [low.net.total / referenceMs, high.net.total / referenceMs] as const;
  const ratio = scores[1] / Math.max(scores[0], LOW_SCORE_FLOOR);
  const before = baseline?.cases[entry.name];
  const comparable =
    before !== undefined && before.scales[0] === small && before.scales[1] === large;
  if (before && !comparable && !update)
    problems.push("scales differ from baseline.json; re-record with --update");

  // Growth: at most 12x per 10x of input, or 1.5x a known case's ratio.
  const step = large / small;
  const stepLimit = MAX_RATIO_PER_10X ** Math.log10(step);
  const limit =
    entry.known && comparable
      ? Math.max(stepLimit, before.ratio * KNOWN_RATIO_TOLERANCE)
      : stepLimit;
  if (ratio > limit && high.net.total > GROWTH_FLOOR_MS)
    problems.push(
      `grows ${ratio.toFixed(1)}x for ${formatStep(step)}x of input (limit ${limit.toFixed(1)})`,
    );

  // Slowdown: at most 1.5x the baseline score, at either scale.
  const slowdowns = comparable
    ? ([0, 1] as const).map((index) => scores[index] / Math.max(before.scores[index]!, 1e-9))
    : [];
  slowdowns.forEach((factor, index) => {
    const ms = scales[index]!.net.total;
    if (factor > MAX_SLOWDOWN && ms > SLOWDOWN_FLOOR_MS)
      problems.push(
        `${entry.scales[index]}x is ${factor.toFixed(2)}x its baseline score (${formatMs(ms)} ms)`,
      );
  });

  const codes = [...new Set([...low.codes, ...high.codes])];
  if (comparable && codes.join(",") !== before.codes.join(","))
    warnings.push(
      `result changed from ${before.codes.join(",") || "accept"} to ${codes.join(",") || "accept"}`,
    );

  if (problems.length > 0 && !update) failures += 1;
  for (const problem of problems)
    annotate(update ? "warning" : "error", `${entry.name}: ${problem}`);
  for (const warning of warnings) annotate("warning", `${entry.name}: ${warning}`);
  const verdict = [
    problems.length === 0 ? "pass" : `FAIL: ${problems.join("; ")}`,
    ...warnings.map((warning) => `warning: ${warning}`),
  ].join("; ");
  const slowdownText = slowdowns.length === 0 ? "-" : `${slowdowns[1]!.toFixed(2)}x`;
  console.log(
    `| ${entry.name} | ${small}, ${large} | ${formatMs(low.net.total)} | ${formatMs(high.net.total)} | ${formatMs(high.net.parse)} | ${formatMs(high.net.check)} | ${ratio.toFixed(1)} per ${formatStep(step)}x (${limit.toFixed(1)})${entry.known ? " known" : ""} | ${slowdownText} | ${verdict} |`,
  );
  recorded[entry.name] = {
    scales: [small, large],
    scores: [Number(scores[0].toFixed(4)), Number(scores[1].toFixed(4))],
    ratio: Number(ratio.toFixed(2)),
    codes,
  };
  report.push({
    case: entry.name,
    scales: entry.scales,
    referenceMs,
    net: [low.net, high.net],
    scores,
    ratio,
    limit,
    slowdowns,
    codes,
    problems,
    warnings,
  });
}

const median = (values: readonly number[]): number =>
  values.toSorted((a, b) => a - b)[Math.floor(values.length / 2)] ?? 0;
const seconds = ((performance.now() - started) / 1000).toFixed(1);
console.log(
  `\nreference program: ${formatMs(median(referenceTimes))} ms (median); gate time ${seconds} s`,
);
if (jsonPath) writeFileSync(jsonPath, `${JSON.stringify(report, null, 2)}\n`);
if (update) {
  const next: Baseline = {
    note: "Recorded by `node --experimental-strip-types test/perf/inference/gate.ts --update`; see README.md.",
    recorded: {
      date: new Date().toISOString().slice(0, 10),
      node: process.version,
      platform: `${process.platform}-${process.arch}`,
    },
    referenceMs: Number(median(referenceTimes).toFixed(1)),
    cases: Object.fromEntries(
      GATE.filter((entry) => recorded[entry.name]).map((entry) => [
        entry.name,
        recorded[entry.name]!,
      ]),
    ),
  };
  writeFileSync(baselinePath, `${JSON.stringify(next, null, 2)}\n`);
  console.log(`recorded ${baselinePath}`);
  process.exit(failures === 0 ? 0 : 1);
}
const total = report.length;
console.log(`${total - failures} of ${total} cases pass`);
process.exit(failures === 0 ? 0 : 1);
