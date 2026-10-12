// errors-per-run: one `hd check` reports every independent mistake, once.
//
// Two files of 10 independent mistakes, each in its own function, marked
// with the conformance line marker `# diagnostic: CODE`:
// - `semantic`: ten type and name errors;
// - `mixed`: eight of those and two syntax errors, so a parser must recover.
// A mistake is reported when its line has a diagnostic. Each once means no
// line has two, and no diagnostic lands on an unmarked line.
// Target (Pillar 1): all N reported, each once. No n/a case.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { diagnosticsOf, runHd } from "../lib/hd.ts";
import { failed, judgeBool, type Metric, type TargetResult } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "errors-per-run";
const TIMEOUT_MS = 60_000;

const SEMANTIC = [
  [
    "fn label(n: i32) -> string:",
    "    let text: string = n  # diagnostic: type-mismatch",
    "    text",
  ],
  ["fn greet() -> string:", "    greeting  # diagnostic: unknown-name"],
  ["fn shout(text: string) -> string:", "    text.shout()  # diagnostic: unknown-method"],
  [
    "fn add(a: i32, b: i32) -> i32:",
    "    a + b",
    "",
    "fn three() -> i32:",
    "    add(1)  # diagnostic: argument-count",
  ],
  ["fn width(text: Strng) -> usize:  # diagnostic: unknown-type", "    0"],
  [
    "fn pieces(text: string) -> List[string]:",
    "    let found: List[string] = []",
    "    found.push(text)  # diagnostic: mutable-receiver-required",
    "    found",
  ],
  [
    "fn sum(values: List[i32]) -> i32:",
    "    total := +0",
    "    for value in values:",
    "        total = total + value  # diagnostic: non-reassignable-binding",
    "    total",
  ],
  [
    "fn grow(base: i64, values: List[i64]) -> i64:",
    "    base + values.len()  # diagnostic: mixed-signedness",
  ],
  [
    "fn hello(name: string?) -> string:",
    '    "hello ${name}"  # diagnostic: unsatisfied-trait-bound',
  ],
  [
    "fn table() -> Map[string, i32]:",
    "    let scores: Map[string, i32] = {}",
    '    scores["ada"] = 1  # diagnostic: readonly-root',
    "    scores",
  ],
];

const SYNTAX = [
  ["fn negate(ready: bool) -> bool:", "    not ready  # diagnostic: syntax-error"],
  ["fn first(pair: (string, i32)) -> string:", "    pair.0  # diagnostic: syntax-error"],
];

export const FILES: Record<string, string[][]> = {
  semantic: SEMANTIC,
  mixed: [SEMANTIC[0]!, SYNTAX[0]!, ...SEMANTIC.slice(2, 6), SYNTAX[1]!, ...SEMANTIC.slice(7)],
};

/** The program text, and the 1-based line of each marked mistake. */
export function assemble(functions: readonly (readonly string[])[]): {
  text: string;
  lines: number[];
} {
  const all = functions.flatMap((body) => [...body, "", ""]);
  const lines = all.flatMap((line, index) =>
    /# diagnostic: [a-z0-9-]+$/.test(line) ? [index + 1] : [],
  );
  return { text: all.join("\n"), lines };
}

export const errorsPerRun: Metric = {
  name: NAME,
  pillar: 1,
  summary: "a file with 10 independent mistakes: each reported once by one hd check",
  async run(context) {
    const dir = makeTempDir(NAME);
    const results: TargetResult[] = [];
    for (const [label, functions] of Object.entries(FILES)) {
      const { text, lines } = assemble(functions);
      const file = `${label}.hd`;
      writeFileSync(join(dir, file), text);
      const name = `${label}: ${lines.length} mistakes reported once`;
      const result = await runHd(context.hd, ["check", "--format", "json", file], {
        cwd: dir,
        timeoutMs: TIMEOUT_MS,
      });
      if (result.timedOut) {
        results.push(failed(NAME, name, "all, once", `timed out after ${TIMEOUT_MS / 1000} s`));
        continue;
      }
      const found = diagnosticsOf(result.stdout).map((d) => Number(d.line));
      const reported = lines.filter((line) => found.includes(line)).length;
      const repeated = lines.filter((line) => found.filter((at) => at === line).length > 1).length;
      const stray = found.filter((at) => !lines.includes(at)).length;
      results.push(
        judgeBool(
          NAME,
          name,
          reported === lines.length && repeated === 0 && stray === 0,
          "all, once",
          `${reported} of ${lines.length}`,
          `${repeated} reported twice or more; ${stray} on other lines`,
        ),
      );
    }
    return results;
  },
};
