// The landing page's claim cards. Every snippet is lines copied from a
// playground example, and every output line is either quoted in that
// example's comments or printed by `hd check` on the stated variant;
// website/test/site.test.ts checks both.

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

export const FEATURES: readonly Feature[] = [
  {
    title: "Effects you can review",
    blurb: "The signature lists everything the function may touch: mail and the clock.",
    contrast: "Elsewhere, only the body tells you it reads the clock or sends mail.",
    example: "effects-in-signature",
    ranges: [
      {
        from: "fn welcome!(email: string) -> void $ Mailer + Clock:  # ← sends mail, reads the clock",
        lines: 4,
      },
    ],
    marks: [
      "fn welcome!(email: string) -> void $ Mailer + Clock:  # ← sends mail, reads the clock",
    ],
  },
  {
    title: "Tests without mocks",
    blurb: "A test installs a fake mailer and a fixed clock for one block.",
    contrast: "Python: `mock.patch` or a DI container.",
    example: "effects-in-signature",
    ranges: [{ from: 'it("welcome mails the user at a fixed time, no mocks"):', lines: 5 }],
    marks: ["$.with(Mailer=outbox, Clock=epoch()):  # ← fakes, for this block only"],
  },
  {
    title: "You can't forget a dependency",
    blurb: "Leave a provider out and the program does not compile.",
    example: "missing-provider",
    ranges: [{ from: "pub fn main!() -> void $ Console:", lines: 5 }],
    marks: ["$.with(Mailer=outbox):  # ← Clock is missing"],
    output: {
      caption: "hd check",
      lines: ["main.hd:9:9: missing-requirement: call to 'welcome' requires Clock"],
      kind: "error",
      source: "check",
    },
  },
  {
    title: "Least authority",
    blurb: "`main` states the whole program's reach: the console, and nothing else.",
    contrast: "Most languages: any imported library can read files or open sockets.",
    example: "least-authority",
    ranges: [
      { from: "pub fn main!() -> void $ Console:  # ← the console, and nothing else", lines: 5 },
    ],
    marks: ["pub fn main!() -> void $ Console:  # ← the console, and nothing else"],
  },
  {
    title: "Exhaustive match",
    blurb: "Add a variant, and the compiler lists every match to update.",
    contrast: "A catch-all arm would have hidden both errors.",
    example: "exhaustive",
    ranges: [{ from: "fn can_cancel(status: OrderStatus) -> bool:", lines: 6 }],
    marks: ["match status:"],
    output: {
      caption: "hd check, after adding Refunded(cents: i32)",
      lines: [
        "main.hd:3:5: nonexhaustive-match: match does not cover: Refunded",
        "main.hd:29:5: nonexhaustive-match: match does not cover: Refunded",
      ],
      kind: "error",
      source: "comment",
    },
  },
  {
    title: "Typed errors with context",
    blurb: "`?` returns the error early, and `.context` says what the program was doing.",
    contrast: "No exceptions: the signature says what can fail.",
    example: "errors",
    ranges: [{ from: "fn start(config: string) -> Result[string, dyn Error]:", lines: 2 }],
    marks: ['n := port(config).context("starting the server")?  # ← early return, with context'],
    output: {
      caption: 'report for "port = eighty"',
      lines: [
        "starting the server",
        "caused by: port 'eighty' is not a number",
        "caused by: invalid digit at position 0",
      ],
      kind: "output",
      source: "comment",
    },
  },
  {
    title: "Property tests find real bugs",
    blurb: "`@derive(Arbitrary)` feeds the test; it found the bug two hand-picked cases missed.",
    example: "derive",
    ranges: [
      { from: "@derive(Arbitrary, Debug, Eq)", lines: 3 },
      { from: 'it_prop("parse undoes print", prop=fn!(m: Money):', lines: 3 },
    ],
    marks: ["@derive(Arbitrary, Debug, Eq)", 'it_prop("parse undoes print", prop=fn!(m: Money):'],
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
    title: "Structured concurrency",
    blurb: "`all!` runs both calls at once and joins the results.",
    example: "concurrency",
    ranges: [{ from: "fn page!(id: i32) -> string $ Clock:", lines: 3 }],
    marks: ["let (profile, orders) = all!(fetch_profile(id), fetch_orders(id))  # ← both at once"],
  },
];

/** The example's text with the feature's variant applied. */
export function featureSource(feature: Feature, examplesDir: string): string {
  const source = readFileSync(join(examplesDir, `${feature.example}.hd`), "utf8");
  if (!feature.variant) return source;
  if (!source.includes(feature.variant.replace))
    throw new Error(`landing page: ${feature.example}.hd has no "${feature.variant.replace}"`);
  return source.replace(feature.variant.replace, feature.variant.with);
}

/** The lines of `source` starting at the one whose trimmed text is `from`, dedented. */
function excerpt(source: string, range: Range, file: string): string {
  const lines = source.split("\n");
  const start = lines.findIndex((line) => line.trim() === range.from);
  if (start < 0) throw new Error(`landing page: ${file} has no line "${range.from}"`);
  const picked = lines.slice(start, start + range.lines);
  const indent = Math.min(...picked.map((line) => /^ */.exec(line)![0].length));
  return picked.map((line) => line.slice(indent)).join("\n");
}

export function featureClaims(
  examplesDir: string,
  playgroundUrl: (code: string) => string,
): Claim[] {
  return FEATURES.map((feature) => {
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
