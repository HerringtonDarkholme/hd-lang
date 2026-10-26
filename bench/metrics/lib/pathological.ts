// Compile-time stress inputs for the `pathological` metric.
//
// Each case renders its template, fixtures/pathological/NAME.hd.tmpl, into
// one single-file hd program at a scale of 1, 2 or 4. The program must
// type-check. Its budget applies at scale 4, the size named in `describe`;
// scales 1 and 2 feed the scaling check, where check time must grow
// near-linearly with size.
//
// The spec lets an implementation limit how deeply source nests
// (grammar.limit.nesting), so the nesting cases stay below 160 levels at
// scale 4: deeper source is a `nesting-too-deep` error, not a stress case.
//
// Usage: node --experimental-strip-types bench/metrics/lib/pathological.ts CASE SCALE
// prints one program, for reading or for trying a case by hand.

import { renderFixture, type TemplateScope } from "./fixtures.ts";

export interface PathologicalCase {
  readonly name: string;
  /** What the case stresses, at scale 4. */
  readonly describe: string;
  /** Wall-time budget of `hd check` at scale 4, in milliseconds. */
  readonly budgetMs: number;
  /** Peak-RSS budget of `hd check` at scale 4, in bytes. */
  readonly budgetBytes: number;
  /** The template's values at a scale. */
  readonly scope: (scale: number) => TemplateScope;
}

const MB = 1024 * 1024;
const DEFAULT_TIME = 2_000;
const DEFAULT_MEMORY = 200 * MB;

const range = (count: number): number[] => Array.from({ length: count }, (_, index) => index);
const budget = { budgetMs: DEFAULT_TIME, budgetBytes: DEFAULT_MEMORY } as const;

export const CASES: readonly PathologicalCase[] = [
  {
    name: "overlapping-from",
    describe:
      "1,600 locals bound by `Cents::from(N)`, each choosing between two `From` impls (F-626)",
    ...budget,
    scope: (scale) => ({ locals: range(400 * scale).map((i) => ({ i })) }),
  },
  {
    name: "deep-blocks",
    describe: "`if` blocks nested 72 deep",
    ...budget,
    scope: (scale) => ({
      levels: range(18 * scale).map((level) => ({ level, indent: "    ".repeat(level + 1) })),
    }),
  },
  {
    name: "deep-parens",
    describe: "an arithmetic expression nested 144 parentheses deep",
    ...budget,
    scope: (scale) => ({ open: "(".repeat(36 * scale), close: " + 1)".repeat(36 * scale) }),
  },
  {
    name: "method-chain",
    describe: "one expression of 2,000 chained string method calls",
    ...budget,
    scope: (scale) => {
      const methods = ["trim()", "lower()", "upper()", "trim_start()", "trim_end()"];
      return {
        chain: range(500 * scale)
          .map((index) => `.${methods[index % methods.length]}`)
          .join(""),
      };
    },
  },
  {
    name: "iterator-chain",
    describe: "one iterator chain of 200 `map` and `filter` stages with typed closures",
    ...budget,
    scope: (scale) => ({
      pairs: range(25 * scale).map((pair) => ({
        add: 2 * pair,
        modulus: ((2 * pair + 1) % 7) + 2,
      })),
    }),
  },
  {
    name: "wide-list-literal",
    describe: "one list literal of 20,000 integers",
    ...budget,
    scope: (scale) => {
      const values = range(5_000 * scale).map((index) => String((index * 7919) % 100_003));
      return {
        rows: range(Math.ceil(values.length / 20)).map((row) => ({
          values: values.slice(row * 20, row * 20 + 20).join(", "),
        })),
      };
    },
  },
  {
    name: "large-enum-match",
    describe: "an enum of 1,000 variants and an exhaustive match over it",
    ...budget,
    scope: (scale) => ({ variants: range(250 * scale).map((i) => ({ i, payload: i % 3 === 0 })) }),
  },
  {
    name: "deep-generic",
    describe: "a generic wrapper instantiated 64 levels deep: Wrap[Wrap[...[i32]]]",
    ...budget,
    scope: (scale) => ({
      wraps: "wrap(".repeat(16 * scale),
      closes: ")".repeat(16 * scale),
      inners: ".inner".repeat(16 * scale),
    }),
  },
  {
    name: "question-chain",
    describe: "one function of 1,000 `?` propagations",
    ...budget,
    scope: (scale) => ({ steps: range(250 * scale).map(() => ({})) }),
  },
  {
    name: "big-file",
    describe: "one file of about 20,000 lines of ordinary functions",
    ...budget,
    scope: (scale) => ({
      functions: range(500 * scale).map((i) => ({ i, modulus: (i % 5) + 2, step: i % 9 })),
    }),
  },
];

/** The program of a case at a scale. */
export const generate = (entry: PathologicalCase, scale: number): string =>
  renderFixture(`pathological/${entry.name}.hd.tmpl`, entry.scope(scale));

/** The one-line program whose check time is the start-up each case subtracts. */
export const STARTUP = "pathological/startup.hd";

if (import.meta.main) {
  const [name, scale] = process.argv.slice(2);
  const found = CASES.find((entry) => entry.name === name);
  if (!found || !scale) {
    console.error(
      `usage: pathological.ts CASE SCALE\ncases: ${CASES.map((entry) => entry.name).join(", ")}`,
    );
    process.exit(2);
  }
  process.stdout.write(generate(found, Number(scale)));
}
