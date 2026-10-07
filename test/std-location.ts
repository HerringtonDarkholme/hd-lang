// Where a test's injected text lands in a std file, read from the file, so
// doc comments added above it don't move the expectation.

import fs from "node:fs";

/**
 * The offset, 1-based line and column of `marker` inside the first `context`
 * in `lib/std/<file>`. Call it before mocking `fs.readFileSync`.
 */
export function stdLocation(
  file: string,
  context: string,
  marker: string,
): { offset: number; line: number; column: number } {
  const source = fs.readFileSync(new URL(`../lib/std/${file}`, import.meta.url), "utf8");
  const start = source.indexOf(context);
  if (start < 0) throw new Error(`lib/std/${file} has no ${JSON.stringify(context)}`);
  const offset = start + context.indexOf(marker);
  const before = source.slice(0, offset);
  return {
    offset,
    line: before.split("\n").length,
    column: offset - before.lastIndexOf("\n"),
  };
}
