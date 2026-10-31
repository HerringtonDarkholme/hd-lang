// errors-per-run: one `hd check` reports every independent mistake, once.
//
// Two files of 10 independent mistakes (fixtures/errors-per-run/), each in
// its own function, marked with the conformance line marker
// `# diagnostic: CODE`:
// - `semantic`: ten type and name errors;
// - `mixed`: eight of those and two syntax errors, so a parser must recover.
// A mistake is reported when its line has a diagnostic. Each once means no
// line has two, and no diagnostic lands on an unmarked line.
// Target (Pillar 1): all N reported, each once. No n/a case.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { fixture } from "../lib/fixtures.ts";
import { diagnosticsOf, runHd } from "../lib/hd.ts";
import { failed, judgeBool, type Metric, type TargetResult } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "errors-per-run";
const TIMEOUT_MS = 60_000;
const MARKER = /# diagnostic: [a-z0-9-]+$/;

/** The mistake files, in fixtures/errors-per-run/. */
export const FILES = ["semantic", "mixed"] as const;

/** The 1-based line of each marked mistake of a program text. */
export const markedLines = (text: string): number[] =>
  text.split("\n").flatMap((line, index) => (MARKER.test(line) ? [index + 1] : []));

export const errorsPerRun: Metric = {
  name: NAME,
  pillar: 1,
  summary: "a file with 10 independent mistakes: each reported once by one hd check",
  async run(context) {
    const dir = makeTempDir(NAME);
    const results: TargetResult[] = [];
    for (const label of FILES) {
      const file = `${label}.hd`;
      const text = fixture(`errors-per-run/${file}`);
      const lines = markedLines(text);
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
      const found = diagnosticsOf(result.stdout).map((item) => Number(item.line));
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
