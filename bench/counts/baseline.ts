// A measurement and its comparison with the merge base's measurement, which
// main's CI stores as an artifact. No expected number is committed.
import { existsSync, readFileSync, writeFileSync } from "node:fs";

export interface Entry {
  program: string;
  retained: string;
  released: string;
  uniques: string;
  total: string;
  // Count calls on an address with no count: paid for, changing nothing. Not in `total`.
  noOps: string;
  // Count calls a call-site test skipped: the test is all they cost.
  skipped: string;
}
export interface Comparison {
  table: string;
  increases: string[];
  decreases: string[];
}

export function countEntry(
  program: string,
  values: readonly bigint[],
  noOps = 0n,
  skipped = 0n,
): Entry {
  const [retained, released, uniques] = values.map((value) => BigInt.asUintN(64, value));
  if (values.length !== 3) {
    throw new Error("expected retain/release/uniqueness counters");
  }
  return {
    program,
    retained: String(retained),
    released: String(released),
    uniques: String(uniques),
    total: String(retained + released + uniques),
    noOps: String(BigInt.asUintN(64, noOps)),
    skipped: String(BigInt.asUintN(64, skipped)),
  };
}

/** The `programs` of a stored `actual.json`; a row that does not add up is refused. */
export function readCounts(text: string): Entry[] {
  const rows = (JSON.parse(text) as { programs?: Entry[] }).programs;
  if (!Array.isArray(rows)) {
    throw new Error("not a count-operation measurement");
  }
  const seen = new Set<string>();
  for (const row of rows) {
    const numbers = [row.retained, row.released, row.uniques, row.total, row.noOps, row.skipped];
    if (
      typeof row.program !== "string" ||
      seen.has(row.program) ||
      numbers.some((number) => typeof number !== "string" || !/^\d+$/.test(number)) ||
      BigInt(row.total) !== BigInt(row.retained) + BigInt(row.released) + BigInt(row.uniques)
    ) {
      throw new Error(`invalid operation counts: ${row.program}`);
    }
    seen.add(row.program);
  }
  return rows;
}

const noOpsAndSkipped = (row: Entry) => BigInt(row.noOps) + BigInt(row.skipped);
const signed = (delta: bigint | number) => `${delta > 0 ? "+" : ""}${delta}`;

/** Sorts one metric's move from `before` to `after` into the comparison's lists. */
export function judge(into: Comparison, what: string, before: bigint, after: bigint): string {
  const line = `${what} ${before} -> ${after} (${signed(after - before)})`;
  if (after > before) {
    into.increases.push(line);
  } else if (after < before) {
    into.decreases.push(line);
  }
  return signed(after - before);
}

/**
 * Each program is judged on its own `total`, and on its no-op and skipped calls,
 * which are outside `total` but still cost time. A cut in one program or metric
 * can't pay for a rise in another. A program the base lacks is new: reported only.
 */
export function compareCounts(actual: readonly Entry[], base?: readonly Entry[]): Comparison {
  const before = new Map(base?.map((row) => [row.program, row]));
  const result: Comparison = { table: "", increases: [], decreases: [] };
  const lines = [
    "| Program | Retains | Releases | Uniqueness | Total | Change | No-op calls | Skipped by a test | Change |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
  ];
  for (const row of actual) {
    const old = before.get(row.program);
    const total = old
      ? judge(result, `${row.program}: count operations`, BigInt(old.total), BigInt(row.total))
      : "new";
    const calls = old
      ? judge(
          result,
          `${row.program}: no-op and skipped calls`,
          noOpsAndSkipped(old),
          noOpsAndSkipped(row),
        )
      : "new";
    lines.push(
      `| ${row.program} | ${row.retained} | ${row.released} | ${row.uniques} | ${row.total} | ${total} | ${row.noOps} | ${row.skipped} | ${calls} |`,
    );
  }
  result.table = lines.join("\n") + "\n";
  return result;
}

/**
 * The verdict both gates share. Without a base it only reports. An increase
 * fails unless a commit in the change carries `Cost-OK: <reason>` (`costOk`).
 */
export function verdict(
  comparison: Comparison,
  hasBase: boolean,
  costOk?: string,
): { text: string; failed: boolean } {
  const list = (lines: string[]) => lines.map((line) => `- ${line}\n`).join("");
  if (!hasBase) {
    return { text: "No stored main measurement to compare with: report only.\n", failed: false };
  }
  let text = comparison.decreases.length
    ? `Decreases from the base:\n\n${list(comparison.decreases)}\n`
    : "";
  const failed = comparison.increases.length > 0 && !costOk?.trim();
  if (!comparison.increases.length) {
    text += "No increase over the base.\n";
  } else if (failed) {
    text += `Increases over the base. Explain them in a \`Cost-OK: <reason>\` line of a commit message in the change:\n\n${list(comparison.increases)}`;
  } else {
    text += `Increases over the base, accepted by \`Cost-OK: ${costOk}\`:\n\n${list(comparison.increases)}`;
  }
  return { text, failed };
}

/**
 * The end of both gates: read the base measurement at `basePath` if there is
 * one, compare, write `summary.md` and fail the process on an unexplained
 * increase. A base this code can't read (another format) is reported only.
 */
export function gate<T>(
  output: string,
  basePath: string | undefined,
  read: (text: string) => T,
  compare: (base?: T) => Comparison,
  costOk?: string,
): void {
  let base: T | undefined;
  if (basePath && existsSync(basePath)) {
    try {
      base = read(readFileSync(basePath, "utf8"));
    } catch (error) {
      process.stderr.write(`::warning::unreadable base ${basePath} (${error}): report only\n`);
    }
  }
  const comparison = compare(base);
  const { text, failed } = verdict(comparison, base !== undefined, costOk);
  writeFileSync(`${output}/summary.md`, `${text}\n${comparison.table}`);
  process.stdout.write(`${text}\n${comparison.table}`);
  if (failed) {
    process.stderr.write(`${comparison.increases.join("\n")}\nno Cost-OK line in the change\n`);
    process.exitCode = 1;
  }
}
