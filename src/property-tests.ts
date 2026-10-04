// The property-test runner (spec/std/testing.md#property-tests, Testing
// T35-T38, T50, T51), the host side of `std.testing.PropertyRunner`
// (spec/std/testing.md#runner-capabilities). An `it_prop` or `it_prop_with`
// test function, whose body is `prop_case!` or `prop_with_case!`
// (lib/std/testing.hd), reports its `cases` and `shrink` caps through
// `start`, and every `Choices` member draws through `draw`, which returns an
// integer from 0 to a bound. The runner records each case's draws as its
// choice stream:
//
// - a new case draws at random: `Choices` offers each draw from its own
//   xoshiro128** generator in hd, which `seed` starts from the run's seed
//   and the case's index, with a reach that `size` grows from case to case
//   (T38);
// - a failing case is shrunk Hypothesis-style (T35): the runner replays
//   shorter or smaller choice streams through the same generator, and a
//   draw past the end of a replayed stream is 0. Each attempt is a fresh
//   instance and counts toward the `shrink` cap (T51);
// - `Choices.assume(false)` calls `discard`, which ends the case as
//   neither passing nor failing. A discarded case does not count toward
//   `cases`, and the property fails after more than 10 × `cases` discards
//   (spec/std/testing.md#r-std-testing.prop.discard-limit);
// - the lowered test reports its input's `Debug` text through `show`,
//   and the failure report prints the shrunk case's text
//   (spec/std/testing.md#r-std-testing.prop.report);
// - `Choices` keeps each case's draw budget itself
//   (spec/std/testing.md#draw-budget);
// - a property's `examples` run first, each as one case: `start` answers
//   which example to run, and discards the case once every example has run
//   (spec/std/testing.md#r-std-testing.prop.examples);
// - a failing property's shrunk stream is saved, one decimal draw per line,
//   under `__regressions__/<module>/<test-slug>` and replayed before new
//   cases on the next run (spec/std/testing.md#r-std-testing.prop.regression-file).
//
// The failure report names the seed, which `hd test --seed N` reuses (T36),
// the shrunk input, and the shrunk choice stream.

import type { HostBoundaryValue } from "./compiler.ts";

/** Thrown by `discard`: the running case is discarded. */
export class PropertyDiscard extends Error {
  constructor() {
    super("the property case was discarded by Choices.assume");
    this.name = "PropertyDiscard";
  }
}

interface PropertyOptions {
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

/** One case's result, as the test runner reports it. */
export type CaseResult = "pass" | "discard" | { readonly failure: string };

/** A `PropertyRunner` argument or answer: an `i64` crosses as a BigInt. */
type RunnerValue = number | bigint | string;

export interface PropertyRun {
  /** Answers a `PropertyRunner` method call of the running case. */
  answer(
    method: string,
    arguments_: readonly RunnerValue[],
    resultType?: string,
  ): { readonly value?: HostBoundaryValue };
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

export function propertyRun(options: PropertyOptions = {}): PropertyRun {
  const seed = options.seed ?? Math.floor(Math.random() * 2147483647);
  let stream: readonly bigint[] = [];
  let recorded: bigint[] = [];
  let caseSeed = seed;
  let size = 0;
  let shrinking = false;
  let caps = { cases: 100, shrink: 500 };
  let shown: string | undefined;
  let example: number | undefined;
  let examplesDone = false;
  const start = (
    next: readonly bigint[],
    nextSeed: number,
    caseSize: number,
    shrinkingCase: boolean,
  ): void => {
    stream = next;
    recorded = [];
    caseSeed = nextSeed;
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
    answer(method, arguments_, resultType) {
      const [first, second, third] = arguments_;
      switch (method) {
        case "start":
          caps = { cases: Number(first), shrink: Number(second) };
          if (example !== undefined && example >= Number(third)) {
            examplesDone = true;
            throw new PropertyDiscard();
          }
          // Keep answering the old std.testing surface until its coordinated
          // switch lands. The declared result type distinguishes that i32
          // answer from the structural PropertyCase answer without naming the
          // std data declaration in the runner.
          if (resultType === "i32") return { value: example ?? -1 };
          return {
            value: {
              example: example === undefined ? { tag: "none" } : { tag: "some", value: example },
              seed: BigInt(caseSeed),
              size,
              replay: [...stream],
            },
          };
        // The case's random draws start from its seed, and reach further
        // as cases grow (Testing T38).
        case "seed":
          return { value: BigInt(caseSeed) };
        case "size":
          return { value: size };
        case "draw": {
          const limit = BigInt(first as bigint);
          const replayed = stream[recorded.length];
          const value =
            replayed !== undefined
              ? replayed < limit
                ? replayed
                : limit
              : shrinking
                ? 0n
                : BigInt(second as bigint);
          const drawn = value < 0n ? 0n : value > limit ? limit : value;
          recorded.push(drawn);
          return { value: drawn };
        }
        case "record":
          recorded.push(first as bigint);
          return {};
        case "discard":
          throw new PropertyDiscard();
        case "show":
          shown = String(first);
          return {};
        default:
          throw new Error(`the property runner has no method ${method}`);
      }
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
