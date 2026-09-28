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
//   neither passing nor failing.
//
// The failure report names the seed, which `hd test --seed N` reuses (T36),
// and the shrunk choice stream. Not decided, so not implemented: printing
// the shrunk value (T36 prints it with `Debug`, but `it_prop` does not bound
// its input by `Debug`), the regression file of T37, and a limit on
// discarded cases; a discarded case counts toward `cases`
// (future-work/TESTING.md, Still Open After T53).

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
}

/** One case's result, as the test runner reports it. */
export type CaseResult = "pass" | "discard" | { readonly failure: string };

export interface PropertyRun {
  readonly hostFunctions: Readonly<Record<string, HostFunction>>;
  readonly seed: number;
  readonly options: PropertyOptions;
  /** Prepares the next case: replay `stream`, then draw from `seed` at `size`, or 0 when shrinking. */
  start(stream: readonly bigint[], seed: number, size: number, shrinking: boolean): void;
  /** The draws of the last case. */
  recorded(): readonly bigint[];
  /** The caps the last case reported. */
  caps(): { readonly cases: number; readonly shrink: number };
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
  return {
    seed,
    options,
    start(next, caseSeed, caseSize, shrinkingCase) {
      stream = next;
      recorded = [];
      random = generator(caseSeed);
      size = caseSize;
      shrinking = shrinkingCase;
    },
    recorded: () => recorded,
    caps: () => caps,
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
 * the test function in a fresh instance with the prepared draws.
 */
export async function runProperty(
  run: PropertyRun,
  name: string,
  once: () => Promise<CaseResult>,
): Promise<{ readonly subject: string; readonly outcome: string } | undefined> {
  for (let index = 0; ; index += 1) {
    run.start([], run.seed + index, index, false);
    const result = await once();
    const cases = run.options.cases ?? run.caps().cases;
    if (typeof result === "object") return shrinkFailure(run, name, index, result, once);
    if (index + 1 >= cases) return undefined;
  }
}

async function shrinkFailure(
  run: PropertyRun,
  name: string,
  index: number,
  first: { readonly failure: string },
  once: () => Promise<CaseResult>,
): Promise<{ readonly subject: string; readonly outcome: string }> {
  const cap = run.options.shrink ?? run.caps().shrink;
  let best = run.recorded();
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
      failure = result.failure;
      improved = true;
      break;
    }
  }
  return {
    subject: `property test "${name}" (seed ${run.seed}, case ${index + 1})`,
    outcome: `${failure}; shrunk choices [${best.join(", ")}] after ${attempts} runs${stoppedEarly ? ", shrinking stopped early" : ""}`,
  };
}
