// The landing page's claim cards: the effect system's three steps, typed
// derivation, and the standard library's testing support. Every snippet is
// lines copied from a playground example, and every output line is either
// quoted in that example's comments or printed by `hd check` on the stated
// variant; website/test/site.test.ts checks both.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { Claim, ClaimOutput } from "./claim.ts";

/** A run of lines in an example: the first line's trimmed text and the count. */
interface Range {
  readonly from: string;
  readonly lines: number;
}

export interface Feature {
  readonly title: string;
  readonly blurb: string;
  readonly contrast?: string;
  /** Playground example file, without `.hd`. */
  readonly example: string;
  /** A one-place edit to the example that the snippet and output show. */
  readonly variant?: { readonly replace: string; readonly with: string };
  /** The snippet's parts, joined by a blank line. */
  readonly ranges: readonly Range[];
  readonly marks: readonly string[];
  /**
   * The output pane. `comment`: each line is quoted in the example. `check`:
   * `hd check` prints each line for the variant saved as main.hd.
   */
  readonly output?: ClaimOutput & { readonly source: "comment" | "check" };
}

/**
 * The effect system, one step per README pillar: requirements declare,
 * providers handle, and `!` suspends. A step's title is its tab label; the
 * home page heads its card with the README's pillar label.
 */
export const EFFECT_STEPS: readonly Feature[] = [
  {
    title: "Require",
    blurb: "The signature lists everything `welcome!` may touch: mail and the clock.",
    contrast: "Elsewhere, only the body tells you it reads the clock or sends mail.",
    example: "effects-in-signature",
    ranges: [
      {
        from: "fn welcome!(email: string) -> void $ Mailer + Clock: # ← sends mail, reads the clock",
        lines: 3,
      },
    ],
    marks: ["fn welcome!(email: string) -> void $ Mailer + Clock: # ← sends mail, reads the clock"],
  },
  {
    title: "Provide",
    blurb: "A test installs a fake mailer and a fixed clock for one block.",
    contrast:
      "Python: `mock.patch` or a DI container. Leave a provider out in hd and it does not compile.",
    example: "effects-in-signature",
    ranges: [{ from: 'it("welcome mails the user at a fixed time, no mocks"):', lines: 4 }],
    marks: ["$.with(Mailer=outbox, Clock=epoch()): # ← fakes, for this block only"],
  },
  {
    title: "Suspend",
    blurb:
      "`page!` suspends, so its name says so. `all!` runs both fetches at once and joins them.",
    contrast:
      "JavaScript: `async` spreads to every caller, and a forgotten `await` still compiles.",
    example: "concurrency",
    ranges: [{ from: "fn page!(id: i32) -> string $ Clock:", lines: 3 }],
    marks: ["let (profile, orders) = all!(fetch_profile(id), fetch_orders(id)) # ← both at once"],
  },
];

/** Typed derivation: one type gives JSON, validation, and a table schema. */
export const DERIVATION: Feature = {
  title: "One type, three derivations",
  blurb: "`@derive` and a `by Structure` block read the type's fields. No macros, no codegen.",
  example: "derive-data",
  ranges: [
    { from: "@derive(Serialize, Deserialize, Table)", lines: 5 },
    { from: "impl Validate for User by Structure:", lines: 2 },
  ],
  marks: ["@derive(Serialize, Deserialize, Table)", "email = [email()]"],
  output: {
    caption: "hd run",
    lines: [
      "SELECT user_id, email FROM users",
      '{"id":7,"email":"ada@example.com"}',
      "$.email: must be a valid email address",
    ],
    kind: "output",
    source: "comment",
  },
};

/** The standard library's help with correctness: effects, fakes, properties, contracts. */
export const STD_FEATURES: readonly Feature[] = [
  {
    title: "Effects in every signature",
    blurb:
      "`read_text!`, `write_text!`, and `now()` each carry a requirement, so `stamp!` must list them.",
    example: "capabilities",
    ranges: [
      {
        from: "fn stamp!(path: Path) -> Result[void, FsError] $ FsRead + FsWrite + Clock: # ← files and the clock",
        lines: 3,
      },
    ],
    marks: [
      "fn stamp!(path: Path) -> Result[void, FsError] $ FsRead + FsWrite + Clock: # ← files and the clock",
    ],
  },
  {
    title: "A fake for every capability",
    blurb:
      "`MemoryFs` and `ManualClock` ship with std. The test touches no disk and no wall clock.",
    contrast: "Same result on every run, on every machine.",
    example: "capabilities",
    ranges: [{ from: 'it("stamps a note in memory at a fixed time"):', lines: 6 }],
    marks: ["$.with(FsRead=files, FsWrite=files, Clock=epoch()): # ← no disk, no wall clock"],
  },
  {
    title: "Property tests from a derive",
    blurb: "`@derive(Arbitrary)` feeds the test; it found the bug two hand-picked cases missed.",
    example: "derive",
    ranges: [
      { from: "@derive(Arbitrary, Debug, Eq)", lines: 3 },
      { from: "it_prop(", lines: 4 },
    ],
    marks: [
      "@derive(Arbitrary, Debug, Eq)",
      'prop=fn!(m: Money): assert_equal(parse(print(m)), .Some(m), reason="round trip")',
    ],
    output: {
      caption: "hd test, on the first version of print",
      lines: [
        'property test "parse undoes print" shrunk input Money { cents: -1000 };',
        "assertion-failed: round trip: actual Option.None, expected Option.Some(Money { cents: -1000 })",
      ],
      kind: "error",
      source: "comment",
    },
  },
  {
    title: "Contracts: require, ensure",
    blurb: "`require` checks the input and `ensure` checks every return, in debug and test builds.",
    example: "contracts",
    ranges: [{ from: "fn withdraw(balance: i64, amount: i64) -> i64:", lines: 7 }],
    marks: ['ensure("an account is never overdrawn"): # ← checked at every return'],
  },
];

/** Every feature on the home page, in page order. */
export const FEATURES: readonly Feature[] = [...EFFECT_STEPS, DERIVATION, ...STD_FEATURES];

/** The example's text with the feature's variant applied. */
export function featureSource(feature: Feature, examplesDir: string): string {
  const source = readFileSync(join(examplesDir, `${feature.example}.hd`), "utf8");
  if (!feature.variant) {
    return source;
  }
  if (!source.includes(feature.variant.replace)) {
    throw new Error(`landing page: ${feature.example}.hd has no "${feature.variant.replace}"`);
  }
  return source.replace(feature.variant.replace, feature.variant.with);
}

/** The lines of `source` starting at the one whose trimmed text is `from`, dedented. */
function excerpt(source: string, range: Range, file: string): string {
  const lines = source.split("\n");
  const start = lines.findIndex((line) => line.trim() === range.from);
  if (start < 0) {
    throw new Error(`landing page: ${file} has no line "${range.from}"`);
  }
  const picked = lines.slice(start, start + range.lines);
  const indent = Math.min(...picked.map((line) => /^ */.exec(line)![0].length));
  return picked.map((line) => line.slice(indent)).join("\n");
}

export function featureClaims(
  features: readonly Feature[],
  examplesDir: string,
  playgroundUrl: (code: string) => string,
): Claim[] {
  return features.map((feature) => {
    const source = featureSource(feature, examplesDir);
    const file = `website/playground/examples/${feature.example}.hd`;
    const original = readFileSync(join(examplesDir, `${feature.example}.hd`), "utf8");
    return {
      title: feature.title,
      blurb: feature.blurb,
      contrast: feature.contrast,
      code: feature.ranges.map((range) => excerpt(source, range, file)).join("\n\n"),
      marks: feature.marks,
      output: feature.output,
      href: playgroundUrl(original),
    };
  });
}
