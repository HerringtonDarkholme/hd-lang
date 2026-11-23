import { judge } from "./baseline.ts";
import type { Comparison } from "./baseline.ts";

export interface ReasonSites {
  reason: string;
  app: number;
  std: number;
  runtime: number;
  sites: number;
}
export interface SiteReport {
  programs: string[];
  reasons: ReasonSites[];
}

const conservative = (reason: string) => reason.startsWith("conservative: ");
const natural = (value: unknown): value is number =>
  Number.isSafeInteger(value) && Number(value) >= 0;

function readPrograms(digestsTsv: string, expectedPrograms: readonly string[]): string[] {
  const programs: string[] = [];
  const expected = new Set(expectedPrograms);
  if (expected.size === 0 || expected.size !== expectedPrograms.length) {
    throw new Error("expected a nonempty, unique benchmark set");
  }
  for (const line of digestsTsv.trimEnd().split("\n")) {
    const [part, program, digest, ...rest] = line.split("\t");
    if (
      rest.length ||
      part !== "bench" ||
      !expected.has(program) ||
      programs.includes(program) ||
      !/^[a-f0-9]{16}$/.test(digest)
    ) {
      throw new Error(`benchmark did not build or report is invalid: ${line}`);
    }
    programs.push(program);
  }
  if (programs.length !== expected.size) {
    throw new Error("count-site report is missing a benchmark");
  }
  return programs.sort();
}

function readTally(line: string, seen: Set<string>) {
  const [part, side, reason, action, count, ...rest] = line.split("\t");
  const sites = Number(count);
  const key = [side, reason, action].join("\t");
  if (
    rest.length ||
    part !== "bench" ||
    !reason ||
    !/^\d+$/.test(count) ||
    !natural(sites) ||
    seen.has(key)
  ) {
    throw new Error(`invalid or duplicate count-reason row: ${line}`);
  }
  seen.add(key);
  if (
    side !== "-" &&
    (!["app", "std", "runtime"].includes(side) ||
      !["retain", "release", "copy", "free", "unique"].includes(action))
  ) {
    throw new Error(`unknown count-reason side or action: ${line}`);
  }
  return { side, reason, action, sites };
}

/** Count emitted sites, including std and generated runtime bodies, not executions. */
export function readSiteReport(
  reasonsTsv: string,
  digestsTsv: string,
  expectedPrograms: readonly string[],
): SiteReport {
  const programs = readPrograms(digestsTsv, expectedPrograms);
  const lines = reasonsTsv.trimEnd().split("\n");
  if (lines.shift() !== "part\tside\treason\taction\tcount") {
    throw new Error("invalid count-reason report header");
  }
  const seen = new Set<string>();
  const rows = new Map<string, ReasonSites>();
  let built = false;
  for (const line of lines) {
    const { side, reason, action, sites } = readTally(line, seen);
    if (side === "-") {
      if (reason !== "built" || action !== "-" || sites !== programs.length || built) {
        throw new Error(`benchmarks were skipped or failed: ${line}`);
      }
      built = true;
      continue;
    }
    const row = rows.get(reason) ?? { reason, app: 0, std: 0, runtime: 0, sites: 0 };
    row[side as "app" | "std" | "runtime"] += sites;
    row.sites += sites;
    if (!natural(row.sites)) {
      throw new Error("count-site sum exceeds integer precision");
    }
    rows.set(reason, row);
  }
  if (!built) {
    throw new Error("count-site report has no successful build total");
  }
  return {
    programs,
    reasons: [...rows.keys()].sort().map((key) => rows.get(key)!),
  };
}

/** A stored `actual.json`; a malformed one is refused. */
export function readSites(text: string): SiteReport {
  const value = JSON.parse(text) as SiteReport;
  if (!Array.isArray(value.programs) || !Array.isArray(value.reasons)) {
    throw new Error("not a conservative-site measurement");
  }
  const seen = new Set<string>();
  for (const row of value.reasons) {
    if (typeof row.reason !== "string" || seen.has(row.reason) || !natural(row.sites)) {
      throw new Error(`invalid or duplicate count-site reason: ${row.reason}`);
    }
    seen.add(row.reason);
  }
  return value;
}

/**
 * Each conservative reason is judged on its own: a fall under one can't hide a
 * rise under another, and a reason the base lacks starts from zero. Named
 * reasons are deliberate and only reported.
 */
export function compareSites(report: SiteReport, base?: SiteReport): Comparison {
  const before = new Map(base?.reasons.map((row) => [row.reason, row.sites]));
  const measured = new Map(report.reasons.map((row) => [row.reason, row]));
  const result: Comparison = { table: "", increases: [], decreases: [] };
  const lines = [
    "### Conservative count sites",
    "",
    `${report.programs.length} benchmarks; static emitted sites, including std and generated runtime bodies.`,
    "",
    "| Reason | App | Std | Runtime | Sites | Base | Change |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
  ];
  let total = 0;
  let fallback = 0;
  for (const reason of [...new Set([...measured.keys(), ...before.keys()])].sort()) {
    const row = measured.get(reason) ?? { reason, app: 0, std: 0, runtime: 0, sites: 0 };
    const old = before.get(reason);
    let change = old === undefined ? "—" : String(row.sites - old);
    if (base && conservative(reason)) {
      change = judge(result, `${reason}: conservative sites`, BigInt(old ?? 0), BigInt(row.sites));
    }
    lines.push(
      `| ${reason} | ${row.app} | ${row.std} | ${row.runtime} | ${row.sites} | ${old ?? "—"} | ${change} |`,
    );
    total += row.sites;
    if (conservative(reason)) {
      fallback += row.sites;
    }
  }
  lines.push("", `**${fallback} conservative / ${total} total sites.**`, "");
  result.table = lines.join("\n");
  return result;
}
