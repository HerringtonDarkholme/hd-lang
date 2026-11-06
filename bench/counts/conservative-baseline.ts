export interface SiteLimit {
  reason: string;
  sites: number;
  explanation?: string;
}
export interface SiteBudget {
  version: 1;
  pipeline: "optimized-release";
  target: "v8";
  reasons: SiteLimit[];
}
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

export function readSiteBudget(text: string): SiteBudget {
  const value = JSON.parse(text) as SiteBudget;
  if (
    value.version !== 1 ||
    value.pipeline !== "optimized-release" ||
    value.target !== "v8" ||
    !Array.isArray(value.reasons)
  ) {
    throw new Error("unsupported conservative count-site budget");
  }
  const seen = new Set<string>();
  for (const row of value.reasons) {
    if (
      typeof row.reason !== "string" ||
      !conservative(row.reason) ||
      row.reason === "conservative: " ||
      seen.has(row.reason) ||
      !natural(row.sites) ||
      (row.explanation !== undefined && typeof row.explanation !== "string")
    ) {
      throw new Error("invalid or duplicate conservative reason budget");
    }
    seen.add(row.reason);
  }
  return value;
}

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

function compareIncreases(
  measured: Map<string, number>,
  budget: SiteBudget,
  previous: SiteBudget,
): string[] {
  const failures: string[] = [];
  const before = new Map(previous.reasons.map((row) => [row.reason, row]));
  for (const row of budget.reasons) {
    const old = before.get(row.reason);
    if (old && row.sites <= old.sites) {
      continue;
    }
    if (!row.explanation?.trim() || row.explanation === old?.explanation) {
      failures.push(`${row.reason}: a new/increased budget needs a fresh explanation`);
    }
    if (row.sites !== (measured.get(row.reason) ?? 0)) {
      failures.push(`${row.reason}: an increased budget must equal the measured site count`);
    }
  }
  return failures;
}

export function compareSites(
  report: SiteReport,
  budget: SiteBudget,
  previous?: SiteBudget,
): string[] {
  const failures: string[] = [];
  const measured = new Map(
    report.reasons.filter((row) => conservative(row.reason)).map((row) => [row.reason, row.sites]),
  );
  const wanted = new Map(budget.reasons.map((row) => [row.reason, row]));
  for (const [reason, sites] of measured) {
    const limit = wanted.get(reason);
    if (!limit || sites > limit.sites) {
      failures.push(
        `${reason}: conservative sites ${limit?.sites ?? 0} -> ${sites}; record the measured increase and its explanation`,
      );
    }
  }
  if (previous) {
    failures.push(...compareIncreases(measured, budget, previous));
  }
  return failures;
}

export function recordSites(
  report: SiteReport,
  previous?: SiteBudget,
  explanation?: string,
): SiteBudget {
  const before = new Map(previous?.reasons.map((row) => [row.reason, row]));
  const measured = new Map(
    report.reasons.filter((row) => conservative(row.reason)).map((row) => [row.reason, row.sites]),
  );
  const keys = new Set([...before.keys(), ...measured.keys()]);
  const reasons = [...keys].sort().map((reason) => {
    const old = before.get(reason);
    const sites = measured.get(reason) ?? 0;
    const increased = previous && (!old || sites > old.sites);
    if (increased && !explanation?.trim()) {
      throw new Error(`${reason}: --reason is required for a new/increased budget`);
    }
    let note = old?.sites === sites ? old.explanation : undefined;
    if (increased) {
      note = explanation;
    }
    return { reason, sites, ...(note ? { explanation: note } : {}) };
  });
  return { version: 1, pipeline: "optimized-release", target: "v8", reasons };
}

export function siteTable(report: SiteReport, budget?: SiteBudget): string {
  const limits = new Map(budget?.reasons.map((row) => [row.reason, row.sites]));
  const measured = new Map(report.reasons.map((row) => [row.reason, row]));
  const keys = new Set([...measured.keys(), ...limits.keys()]);
  const lines = [
    "### Conservative count sites",
    "",
    `${report.programs.length} benchmarks; static emitted sites, including std and generated runtime bodies.`,
    "",
    "| Reason | App | Std | Runtime | Sites | Budget/base | Change |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
  ];
  let total = 0;
  let fallback = 0;
  for (const reason of [...keys].sort()) {
    const row = measured.get(reason) ?? { reason, app: 0, std: 0, runtime: 0, sites: 0 };
    const limit = limits.get(reason);
    const change = limit === undefined ? "—" : String(row.sites - limit);
    lines.push(
      `| ${reason} | ${row.app} | ${row.std} | ${row.runtime} | ${row.sites} | ${limit ?? "—"} | ${change} |`,
    );
    total += row.sites;
    if (conservative(reason)) {
      fallback += row.sites;
    }
  }
  lines.push("", `**${fallback} conservative / ${total} total sites.**`, "");
  return lines.join("\n");
}
