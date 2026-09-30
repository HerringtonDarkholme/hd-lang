import type { HostFunction } from "./host-functions.ts";

// The property-test runner (spec/10-modules.md#property-tests, Testing
// T35-T38, T50, T51). A lowered `it_prop` or `it_prop_with` test function
// (parser/test-cases.ts) reports its `cases` and `shrink` caps through
// `prop_config`, and every `Choices` member draws through `prop_draw`, a
// host function that returns an integer from 0 to a bound. The runner
// records each case's draws as its choice stream:
//
// - a new case draws at random from the run's seed, with a size that grows
//   from case to case (T38);
// - a failing case is shrunk Hypothesis-style (T35): the runner replays
//   shorter or smaller choice streams through the same generator, and a
//   draw past the end of a replayed stream is 0. Each attempt is a fresh
//   instance and counts toward the `shrink` cap (T51);
// - `Choices.assume(false)` calls `prop_discard`, which ends the case as
//   neither passing nor failing. A discarded case does not count toward
//   `cases`, and the property fails after more than 10 × `cases` discards
//   (spec/10-modules.md#r-module.testing.prop.discard-limit);
// - the lowered test reports its input's `Debug` text through `prop_show`,
//   and the failure report prints the shrunk case's text
//   (spec/10-modules.md#r-module.testing.prop.report);
// - each case has a draw budget of `DRAW_BUDGET` draws, which `prop_budget`
//   reports; once `Choices` has spent it, every draw returns its simplest
//   value without calling `prop_draw` (spec/10-modules.md#draw-budget);
// - a property's `examples` run first, each as one case: the lowered test
//   asks `prop_example` which example to run, and the host discards the
//   case once every example has run (spec/10-modules.md#r-module.testing.prop.examples);
// - a failing property's shrunk stream is saved, one decimal draw per line,
//   under `__regressions__/<module>/<test-slug>` and replayed before new
//   cases on the next run (spec/10-modules.md#r-module.testing.prop.regression-file).
//
// The failure report names the seed, which `hd test --seed N` reuses (T36),
// the shrunk input, and the shrunk choice stream.

/** Thrown by `prop_discard`: the running case is discarded. */
export class PropertyDiscard extends Error {
  constructor() {
    super("the property case was discarded by Choices.assume");
    this.name = "PropertyDiscard";
  }
}

export interface PropertyOptions {
  readonly seed?: number;
  readonly cases?: number;
  readonly shrink?: number;
  /** Where failing streams are saved and replayed from; none keeps nothing. */
  readonly regressions?: RegressionStore;
}

/** The saved choice streams of failing properties, by test name (src/snapshots.ts). */
export interface RegressionStore {
  load(name: string): readonly bigint[] | undefined;
  /** Saves `stream` and returns the file's path from the package root. */
  save(name: string, stream: readonly bigint[]): string;
}

/** The draws of one case before every draw returns its simplest value. */
export const DRAW_BUDGET = 256;

/** One case's result, as the test runner reports it. */
export type CaseResult = "pass" | "discard" | { readonly failure: string };

export interface PropertyRun {
  readonly hostFunctions: Readonly<Record<string, HostFunction>>;
  readonly seed: number;
  readonly options: PropertyOptions;
  /** Prepares the next case: replay `stream`, then draw from `seed` at `size`, or 0 when shrinking. */
  start(stream: readonly bigint[], seed: number, size: number, shrinking: boolean): void;
  /** Prepares a case that runs example `index`. */
  startExample(index: number): void;
  /** Whether the last example case found no example left to run. */
  examplesDone(): boolean;
  /** The draws of the last case. */
  recorded(): readonly bigint[];
  /** The caps the last case reported. */
  caps(): { readonly cases: number; readonly shrink: number };
  /** The `Debug` text of the last case's input, when it reported one. */
  shown(): string | undefined;
}

/** A small seeded generator (mulberry32). */
function generator(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

/** A random draw from 0 to `cap`: sometimes an end of the range, else uniform. */
function randomDraw(random: () => number, cap: bigint): bigint {
  if (cap <= 0n) return 0n;
  const pick = random();
  if (pick < 0.1) return 0n;
  if (pick < 0.15) return cap;
  const high = BigInt(Math.floor(random() * 4294967296));
  const low = BigInt(Math.floor(random() * 4294967296));
  return ((high << 32n) | low) % (cap + 1n);
}

export function propertyRun(options: PropertyOptions = {}): PropertyRun {
  const seed = options.seed ?? Math.floor(Math.random() * 2147483647);
  let stream: readonly bigint[] = [];
  let recorded: bigint[] = [];
  let random = generator(seed);
  let size = 0;
  let shrinking = false;
  let caps = { cases: 100, shrink: 500 };
  let shown: string | undefined;
  let example: number | undefined;
  let examplesDone = false;
  const start = (
    next: readonly bigint[],
    caseSeed: number,
    caseSize: number,
    shrinkingCase: boolean,
  ): void => {
    stream = next;
    recorded = [];
    random = generator(caseSeed);
    size = caseSize;
    shrinking = shrinkingCase;
    shown = undefined;
    example = undefined;
  };
  return {
    seed,
    options,
    start,
    startExample(index) {
      start([], seed, 0, true);
      example = index;
      examplesDone = false;
    },
    examplesDone: () => examplesDone,
    recorded: () => recorded,
    caps: () => caps,
    shown: () => shown,
    hostFunctions: {
      prop_config(cases, shrink) {
        caps = { cases: Number(cases), shrink: Number(shrink) };
      },
      prop_draw(bound) {
        const limit = BigInt(bound as bigint | number);
        const index = recorded.length;
        // Sizes grow from small to large (Testing T38).
        const sized = 2n ** BigInt(Math.min(62, 3 + size));
        const cap = limit < sized ? limit : sized;
        const replayed = stream[index];
        const value =
          replayed !== undefined
            ? replayed < limit
              ? replayed
              : limit
            : shrinking
              ? 0n
              : randomDraw(random, cap);
        const drawn = value < 0n ? 0n : value;
        recorded.push(drawn);
        return drawn;
      },
      prop_discard() {
        throw new PropertyDiscard();
      },
      prop_budget() {
        return DRAW_BUDGET;
      },
      prop_example(count) {
        if (example === undefined) return -1;
        if (example < Number(count)) return example;
        examplesDone = true;
        throw new PropertyDiscard();
      },
      prop_show(text) {
        shown = String(text);
      },
    },
  };
}

/** Whether `left` is a simpler choice stream than `right`: shorter, then smaller. */
function simpler(left: readonly bigint[], right: readonly bigint[]): boolean {
  if (left.length !== right.length) return left.length < right.length;
  for (let index = 0; index < left.length; index += 1)
    if (left[index] !== right[index]) return left[index]! < right[index]!;
  return false;
}

/** Smaller streams to try: deleted runs of draws, then zeroed and halved draws. */
function* candidates(stream: readonly bigint[]): Generator<readonly bigint[]> {
  for (const width of [8, 4, 2, 1])
    for (let start = stream.length - width; start >= 0; start -= 1)
      yield [...stream.slice(0, start), ...stream.slice(start + width)];
  for (let index = 0; index < stream.length; index += 1) {
    const value = stream[index]!;
    for (const smaller of [0n, value / 2n, value - 1n])
      if (smaller >= 0n && smaller < value)
        yield stream.map((item, position) => (position === index ? smaller : item));
  }
}

/**
 * Runs one property test case by case, then shrinks a failure. `once` runs
 * the test function in a fresh instance with the prepared draws. The
 * examples run first, then a saved regression stream; discarded cases do
 * not count toward `cases`.
 */
export async function runProperty(
  run: PropertyRun,
  name: string,
  once: () => Promise<CaseResult>,
): Promise<{ readonly subject: string; readonly outcome: string } | undefined> {
  for (let example = 0; ; example += 1) {
    run.startExample(example);
    const result = await once();
    if (run.examplesDone()) break;
    if (typeof result === "object")
      return {
        subject: `property test "${name}" (seed ${run.seed}, example ${example + 1})`,
        outcome: [
          ...(run.shown() === undefined ? [] : [`input ${run.shown()}`]),
          result.failure,
        ].join("; "),
      };
  }
  const saved = run.options.regressions?.load(name);
  if (saved !== undefined) {
    run.start(saved, run.seed, 0, true);
    const result = await once();
    if (typeof result === "object")
      return shrinkFailure(run, name, 0, "the saved regression case", result, once);
  }
  let checked = 0;
  let discarded = 0;
  for (let index = 0; ; index += 1) {
    run.start([], run.seed + index, checked, false);
    const result = await once();
    const cases = run.options.cases ?? run.caps().cases;
    if (typeof result === "object")
      return shrinkFailure(run, name, index, `case ${index + 1}`, result, once);
    if (result === "discard") {
      discarded += 1;
      if (discarded > 10 * cases)
        return {
          subject: `property test "${name}" (seed ${run.seed})`,
          outcome: `discarded ${discarded} cases, more than 10 × cases (${cases}), after ${checked} checked cases`,
        };
      continue;
    }
    checked += 1;
    if (checked >= cases) return undefined;
  }
}

async function shrinkFailure(
  run: PropertyRun,
  name: string,
  index: number,
  label: string,
  first: { readonly failure: string },
  once: () => Promise<CaseResult>,
): Promise<{ readonly subject: string; readonly outcome: string }> {
  const cap = run.options.shrink ?? run.caps().shrink;
  let best = run.recorded();
  let input = run.shown();
  let failure = first.failure;
  let attempts = 0;
  let stoppedEarly = false;
  let improved = true;
  while (improved && !stoppedEarly) {
    improved = false;
    for (const candidate of candidates(best)) {
      if (attempts >= cap) {
        stoppedEarly = true;
        break;
      }
      attempts += 1;
      run.start(candidate, run.seed + index, index, true);
      const result = await once();
      if (typeof result !== "object" || !simpler(run.recorded(), best)) continue;
      best = run.recorded();
      input = run.shown();
      failure = result.failure;
      improved = true;
      break;
    }
  }
  const saved = run.options.regressions?.save(name, best);
  return {
    subject: `property test "${name}" (seed ${run.seed}, ${label})`,
    outcome: [
      ...(input === undefined ? [] : [`shrunk input ${input}`]),
      failure,
      ...(saved === undefined ? [] : [`saved to ${saved}`]),
      `shrunk choices [${best.join(", ")}] after ${attempts} runs${stoppedEarly ? ", shrinking stopped early" : ""}`,
    ].join("; "),
  };
}
