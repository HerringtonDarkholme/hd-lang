// Child process of run.ts and gate.ts: times the parse and the check of one
// case at one or more scales, net of the fixed cost of an empty program (the
// standard library, which the checker joins to every program).
//
// Usage: node --experimental-strip-types test/perf/inference/measure.ts
//   CASE SCALE... [--runs N] [--reference SCALE] [--emit]
// prints one JSON line (see Report below).
//
// Every timed round runs the empty program, the reference program (with
// --reference), and each scale once, in that order, so slow drift of the
// machine hits all of them alike. The process reports the median of each.
// A program whose first round takes over 300 ms is timed in three rounds at
// most, and over two seconds in the first round only.
import { resolve } from "node:path";

import { CASES, REFERENCE } from "./gen.ts";

const root = resolve(import.meta.dirname, "../../..");
interface Diagnostic {
  readonly code: string;
  readonly severity?: string;
}
// The compiler's exported phase entry points; src/compiler.ts's `analyze` is
// `parse` then `check`, and `compileToWat` adds `emitWat`.
const { parse } = (await import(resolve(root, "src/parser/index.ts"))) as {
  parse: (source: string) => { program?: unknown; diagnostics: readonly Diagnostic[] };
};
const { check } = (await import(resolve(root, "src/checker/index.ts"))) as {
  check: (program: unknown) => { program?: unknown; diagnostics: readonly Diagnostic[] };
};
const { emitWat } = (await import(resolve(root, "src/emitter/index.ts"))) as {
  emitWat: (program: unknown) => string;
};
const { DiagnosticError } = (await import(resolve(root, "src/diagnostics.ts"))) as {
  DiagnosticError: new (...args: never[]) => Error & { diagnostics: readonly Diagnostic[] };
};

/** Milliseconds per phase; `emit` is 0 without --emit. */
export interface Phases {
  readonly parse: number;
  readonly check: number;
  readonly emit: number;
  readonly total: number;
}

export interface Report {
  /** The empty program's median phases, subtracted from every other time. */
  readonly empty: Phases;
  /** The reference program's net median phases (with --reference). */
  readonly reference?: Phases;
  readonly scales: readonly { scale: number; net: Phases; runs: number; codes: string[] }[];
}

const positional: string[] = [];
let runs = 5;
let referenceScale: number | undefined;
let emit = false;
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 1) {
  const option = args[index]!;
  if (option === "--runs") runs = Number(args[++index]);
  else if (option === "--reference") referenceScale = Number(args[++index]);
  else if (option === "--emit") emit = true;
  else positional.push(option);
}
const [name, ...scaleTexts] = positional;
const found = [...CASES, REFERENCE].find((entry) => entry.name === name);
if (!found || scaleTexts.length === 0 || !(runs >= 1)) {
  console.error("usage: measure.ts CASE SCALE... [--runs N] [--reference SCALE] [--emit]");
  process.exit(2);
}

const errorCodes = (diagnostics: readonly Diagnostic[]): string[] =>
  diagnostics.filter((entry) => entry.severity !== "warning").map((entry) => entry.code);

const time = (program: string): { phases: Phases; codes: string[] } => {
  const start = performance.now();
  let parsed = start;
  let checked = start;
  let codes: string[] = [];
  try {
    const result = parse(program);
    parsed = performance.now();
    codes = errorCodes(result.diagnostics);
    if (result.program) {
      const typed = check(result.program);
      checked = performance.now();
      codes.push(...errorCodes(typed.diagnostics));
      if (emit && typed.program && codes.length === 0) emitWat(typed.program);
    }
  } catch (error) {
    if (!(error instanceof DiagnosticError)) throw error;
    codes.push(...errorCodes(error.diagnostics));
  }
  const end = performance.now();
  if (parsed === start) parsed = end;
  if (checked === start) checked = Math.max(parsed, end);
  return {
    phases: {
      parse: parsed - start,
      check: checked - parsed,
      emit: end - checked,
      total: end - start,
    },
    codes,
  };
};

const median = (values: readonly number[]): number => {
  const sorted = values.toSorted((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[middle]! : (sorted[middle - 1]! + sorted[middle]!) / 2;
};
const PHASES = ["parse", "check", "emit", "total"] as const;
const medianPhases = (samples: readonly Phases[]): Phases =>
  Object.fromEntries(
    PHASES.map((phase) => [phase, median(samples.map((sample) => sample[phase]))]),
  ) as unknown as Phases;
const net = (phases: Phases, empty: Phases): Phases =>
  Object.fromEntries(
    PHASES.map((phase) => [phase, Math.max(0, phases[phase] - empty[phase])]),
  ) as unknown as Phases;

interface Program {
  readonly source: string;
  readonly samples: Phases[];
  codes: string[];
}
const program = (source: string): Program => ({ source, samples: [], codes: [] });
const empty = program("fn run() -> void:\n    pass\n");
const reference =
  referenceScale === undefined ? undefined : program(REFERENCE.generate(referenceScale));
const sized = scaleTexts.map((text) => ({
  scale: Number(text),
  ...program(found.generate(Number(text))),
}));
const all = [empty, ...(reference ? [reference] : []), ...sized];

// One untimed round of the small programs warms the module and JIT caches.
for (const entry of [empty, ...(reference ? [reference] : [])]) time(entry.source);
for (let round = 0; round < runs; round += 1)
  for (const entry of all) {
    const first = entry.samples[0]?.total ?? 0;
    if ((round >= 1 && first > 2000) || (round >= 3 && first > 300)) continue;
    const { phases, codes } = time(entry.source);
    entry.samples.push(phases);
    if (round === 0) entry.codes = [...new Set(codes)];
  }

const emptyPhases = medianPhases(empty.samples);
const report: Report = {
  empty: emptyPhases,
  ...(reference ? { reference: net(medianPhases(reference.samples), emptyPhases) } : {}),
  scales: sized.map((entry) => ({
    scale: entry.scale,
    net: net(medianPhases(entry.samples), emptyPhases),
    runs: entry.samples.length,
    codes: entry.codes,
  })),
};
console.log(JSON.stringify(report));
