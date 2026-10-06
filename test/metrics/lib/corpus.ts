// The single-mistake corpus in test/metrics/mistakes/.
//
// Each program holds exactly one mistake, taken from audit/hd-writing-log.md.
// Its header comments say what the mistake is and which code the
// specification gives it; the mistake's line ends in the conformance line
// marker `# diagnostic: CODE` (or `# warning: CODE`), as in
// spec/conformance/README.md:
//
//   # mistake: Python's `not` for logical negation
//   # log: 2026-09-29 property-test trial
//   # expect: syntax-error
//   # fixed:     !ready
//
//   fn negate(ready: bool) -> bool:
//       not ready  # diagnostic: syntax-error
//
// `# fixed:` is optional: the corrected text of the marked line, where `\n`
// starts another line; `# fixed-lines: N` makes it replace N lines from the
// marked one. The corpus check uses it to confirm that the program
// holds no other mistake; no metric reads it as the expected fix-it.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export interface Mistake {
  /** The file name without `.hd`. */
  readonly name: string;
  readonly file: string;
  readonly text: string;
  readonly mistake: string;
  readonly log: string;
  /** The code the specification gives the mistake. */
  readonly code: string;
  readonly severity: "error" | "warning";
  /** The 1-based line of the mistake. */
  readonly line: number;
  /** The corrected program, when the header gives one. */
  readonly fixedText?: string;
}

const header = (text: string, name: string): string | undefined =>
  new RegExp(`^# ${name}: (.*)$`, "m").exec(text)?.[1];

export function parseMistake(name: string, file: string, text: string): Mistake {
  const lines = text.split("\n");
  const code = header(text, "expect");
  const mistake = header(text, "mistake");
  const log = header(text, "log");
  if (!code || !mistake || !log) throw new Error(`${file}: needs mistake, log, and expect headers`);
  const marked = lines
    .map((line, index) => ({
      index,
      match: /#\s*(diagnostic|warning): ([a-z0-9-]+)\s*$/.exec(line),
    }))
    .filter((entry) => entry.match && !lines[entry.index]!.startsWith("#"));
  if (marked.length !== 1) throw new Error(`${file}: needs exactly one line marker`);
  const [{ index, match }] = marked as [{ index: number; match: RegExpExecArray }];
  if (match[2] !== code) throw new Error(`${file}: marker ${match[2]} differs from expect ${code}`);
  const fixed = header(text, "fixed");
  const span = Number(header(text, "fixed-lines") ?? "1");
  const fixedText =
    fixed === undefined
      ? undefined
      : [...lines.slice(0, index), ...fixed.split("\\n"), ...lines.slice(index + span)].join("\n");
  return {
    name,
    file,
    text,
    mistake,
    log,
    code,
    severity: match[1] === "warning" ? "warning" : "error",
    line: index + 1,
    ...(fixedText === undefined ? {} : { fixedText }),
  };
}

/** Every program of the corpus directory, sorted by name. */
export function loadMistakes(dir: string): Mistake[] {
  return readdirSync(dir)
    .filter((entry) => entry.endsWith(".hd"))
    .sort()
    .map((entry) => {
      const file = join(dir, entry);
      return parseMistake(entry.slice(0, -3), file, readFileSync(file, "utf8"));
    });
}
