// The full suite: times the check of each pathological inference case
// (gen.ts) at 1x, 10x, and 100x, and fails a case whose time grows
// super-linearly. gate.ts is the trimmed version that CI runs.
//
// Usage: node --experimental-strip-types test/perf/inference/run.ts
//   [--case NAME]... [--timeout SECONDS] [--emit] [--json FILE]
//
// Each size runs in its own process with a timeout (default 120 s). Times
// are net of an empty program's check. A growth ratio above 12 between
// adjacent sizes fails the case, unless the larger time is under the noise
// floor of 250 ms. A timeout or a crash fails the case and skips its larger
// sizes. A result other than the case's expected one (gen.ts `expect`, which
// follows the spec) is shown but does not fail: on main the prototype rejects
// several cases (test/portable/KNOWN_FAILURES.tsv, literal-first-use and
// literal-var rows).
import { writeFileSync } from "node:fs";

import { CASES } from "./gen.ts";
import { formatMs, measureInChild } from "./spawn.ts";

const MAX_RATIO = 12;
const NOISE_FLOOR_MS = 250;
const SCALES = [1, 10, 100] as const;

interface Measurement {
  readonly scale: number;
  readonly ms?: number;
  readonly parseMs?: number;
  readonly status: "ok" | "timeout" | "crash";
  readonly detail?: string;
  readonly codes?: readonly string[];
}

const args = process.argv.slice(2);
const selected: string[] = [];
let timeoutSeconds = 120;
let emit = false;
let jsonPath: string | undefined;
for (let index = 0; index < args.length; index += 1) {
  const option = args[index];
  if (option === "--case") selected.push(args[++index]!);
  else if (option === "--timeout") timeoutSeconds = Number(args[++index]);
  else if (option === "--emit") emit = true;
  else if (option === "--json") jsonPath = args[++index];
  else {
    console.error(`unknown option ${option}`);
    process.exit(2);
  }
}

const cases = CASES.filter((entry) => selected.length === 0 || selected.includes(entry.name));

const measure = (name: string, scale: number): Measurement => {
  const outcome = measureInChild(
    [name, String(scale), "--runs", "3", ...(emit ? ["--emit"] : [])],
    timeoutSeconds,
  );
  if (outcome.status !== "ok")
    return { scale, status: outcome.status, ...(outcome.detail ? { detail: outcome.detail } : {}) };
  const { net, codes } = outcome.report.scales[0]!;
  return { scale, ms: net.total, parseMs: net.parse, status: "ok", codes };
};

const report: Record<string, unknown>[] = [];
let failures = 0;
console.log("| case | 1x ms | 10x ms | 100x ms | 10x/1x | 100x/10x | result | verdict |");
console.log("| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |");
for (const entry of cases) {
  const measurements: Measurement[] = [];
  for (const scale of SCALES) {
    const measurement = measure(entry.name, scale);
    measurements.push(measurement);
    if (measurement.status !== "ok") break;
  }
  const ratio = (larger?: Measurement, smaller?: Measurement): number | undefined =>
    larger?.ms !== undefined && smaller?.ms !== undefined
      ? larger.ms / Math.max(smaller.ms, 1)
      : undefined;
  const ratios = [ratio(measurements[1], measurements[0]), ratio(measurements[2], measurements[1])];
  const problems: string[] = [];
  for (const measurement of measurements)
    if (measurement.status !== "ok")
      problems.push(
        `${measurement.scale}x ${measurement.status}${measurement.detail ? ` (${measurement.detail})` : ""}`,
      );
  ratios.forEach((value, index) => {
    const larger = measurements[index + 1];
    if (value !== undefined && value > MAX_RATIO && (larger?.ms ?? 0) > NOISE_FLOOR_MS)
      problems.push(`super-linear ${SCALES[index + 1]}x/${SCALES[index]}x = ${value.toFixed(1)}`);
  });
  if (problems.length > 0) failures += 1;
  const codes = [...new Set(measurements.flatMap((measurement) => measurement.codes ?? []))];
  const result = codes.length === 0 ? "accept" : `reject:${codes.join(",")}`;
  const cell = (scale: number): string => {
    const measurement = measurements.find((candidate) => candidate.scale === scale);
    if (!measurement) return "skipped";
    if (measurement.status === "timeout") return `>${timeoutSeconds}000`;
    if (measurement.status === "crash") return "crash";
    return formatMs(measurement.ms);
  };
  console.log(
    `| ${entry.name} | ${cell(1)} | ${cell(10)} | ${cell(100)} | ${formatMs(ratios[0])} | ${formatMs(ratios[1])} | ${
      result === entry.expect ? result : `${result} (spec: ${entry.expect})`
    } | ${problems.length === 0 ? "pass" : `FAIL: ${problems.join("; ")}`} |`,
  );
  report.push({ case: entry.name, measurements, ratios, result, problems });
}
if (jsonPath) writeFileSync(jsonPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(`\n${cases.length - failures} of ${cases.length} cases pass`);
process.exit(failures === 0 ? 0 : 1);
