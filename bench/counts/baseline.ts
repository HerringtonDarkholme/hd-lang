export interface Counts {
  retained: string;
  released: string;
  uniques: string;
  total: string;
}
export interface Entry extends Counts {
  program: string;
  // Count calls on an address with no count: paid for, changing nothing. Not in `total`.
  noOps: string;
  // Count calls a call-site test skipped: the test is all they cost.
  skipped: string;
}
export interface Recorded extends Counts {
  program: string;
  reason?: string;
  // `noOps` + `skipped`, limited like `total`. A baseline older than this limit has none.
  noOpsAndSkipped?: string;
}
export interface Baseline {
  version: 1;
  pipeline: "optimized-release-checked";
  engine: "node-default";
  programs: Recorded[];
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

function noOpsAndSkipped(row: Entry): string {
  return String(BigInt(row.noOps) + BigInt(row.skipped));
}

function totalRose(row: Counts, old?: Counts): boolean {
  return !old || BigInt(row.total) > BigInt(old.total);
}

function noOpsRose(row: Recorded, old?: Recorded): boolean {
  if (!old) {
    return true;
  }
  return (
    row.noOpsAndSkipped !== undefined &&
    old.noOpsAndSkipped !== undefined &&
    BigInt(row.noOpsAndSkipped) > BigInt(old.noOpsAndSkipped)
  );
}

export function readBaseline(text: string): Baseline {
  const value = JSON.parse(text) as Baseline;
  if (
    value.version !== 1 ||
    value.pipeline !== "optimized-release-checked" ||
    value.engine !== "node-default" ||
    !Array.isArray(value.programs)
  ) {
    throw new Error("unsupported count-operation baseline");
  }
  const seen = new Set<string>();
  for (const row of value.programs) {
    if (typeof row.program !== "string" || !row.program || seen.has(row.program)) {
      throw new Error("missing or duplicate program in count baseline");
    }
    seen.add(row.program);
    const numbers = [row.retained, row.released, row.uniques, row.total];
    if (
      numbers.some((number) => typeof number !== "string" || !/^\d+$/.test(number)) ||
      BigInt(row.total) !== BigInt(row.retained) + BigInt(row.released) + BigInt(row.uniques) ||
      (row.reason !== undefined && typeof row.reason !== "string") ||
      (row.noOpsAndSkipped !== undefined && !/^\d+$/.test(String(row.noOpsAndSkipped)))
    ) {
      throw new Error(`invalid operation counts: ${row.program}`);
    }
  }
  return value;
}

export function compareCounts(
  actual: readonly Entry[],
  recorded: Baseline,
  previous?: Baseline,
): string[] {
  const failures: string[] = [];
  const wanted = new Map(recorded.programs.map((row) => [row.program, row]));
  const measured = new Map(actual.map((row) => [row.program, row]));
  for (const row of actual) {
    const limit = wanted.get(row.program);
    if (!limit) {
      failures.push(`${row.program}: missing baseline`);
    } else if (BigInt(row.total) > BigInt(limit.total)) {
      failures.push(
        `${row.program}: count operations ${limit.total} -> ${row.total}; record the measured increase and its reason`,
      );
    }
    if (limit && limit.noOpsAndSkipped === undefined) {
      failures.push(`${row.program}: baseline has no limit for no-op and skipped calls`);
    } else if (limit && BigInt(noOpsAndSkipped(row)) > BigInt(limit.noOpsAndSkipped!)) {
      failures.push(
        `${row.program}: no-op and skipped calls ${limit.noOpsAndSkipped} -> ${noOpsAndSkipped(row)}; record the measured increase and its reason`,
      );
    }
  }
  for (const row of recorded.programs) {
    if (!measured.has(row.program)) {
      failures.push(`${row.program}: baseline has no measured program`);
    }
  }
  if (previous) {
    failures.push(...checkIncreaseExplanations(measured, recorded, previous));
  }
  return failures;
}

function checkIncreaseExplanations(
  measured: Map<string, Entry>,
  recorded: Baseline,
  previous: Baseline,
): string[] {
  const failures: string[] = [];
  const before = new Map(previous.programs.map((row) => [row.program, row]));
  for (const row of recorded.programs) {
    const old = before.get(row.program);
    const totalUp = totalRose(row, old);
    const noOpsUp = noOpsRose(row, old);
    if (!totalUp && !noOpsUp) {
      continue;
    }
    if (!row.reason?.trim() || row.reason === old?.reason) {
      failures.push(
        `${row.program}: a new/increased baseline needs a new explanation in its reason field`,
      );
    }
    const now = measured.get(row.program);
    if (totalUp && now?.total !== row.total) {
      failures.push(`${row.program}: an increased baseline must equal the measured total`);
    }
    if (noOpsUp && (!now || noOpsAndSkipped(now) !== row.noOpsAndSkipped)) {
      failures.push(`${row.program}: an increased no-op limit must equal the measured calls`);
    }
  }
  return failures;
}

export function recordCounts(actual: Entry[], previous?: Baseline, reason?: string): Baseline {
  const before = new Map(previous?.programs.map((row) => [row.program, row]));
  const programs = actual.map((row) => {
    const old = before.get(row.program);
    const { noOps: _noOps, skipped: _skipped, ...counts } = row;
    const next: Recorded = { ...counts, noOpsAndSkipped: noOpsAndSkipped(row) };
    const increased = previous && (totalRose(next, old) || noOpsRose(next, old));
    if (increased && !reason?.trim()) {
      throw new Error(`${row.program}: --reason is required for an increase or a new benchmark`);
    }
    const same =
      old?.total === next.total &&
      (old.noOpsAndSkipped ?? next.noOpsAndSkipped) === next.noOpsAndSkipped;
    const kept = same ? old?.reason : undefined;
    const note = increased ? reason : kept;
    return { ...next, ...(note ? { reason: note } : {}) };
  });
  return { version: 1, pipeline: "optimized-release-checked", engine: "node-default", programs };
}

export function countTable(actual: readonly Entry[], recorded?: Baseline): string {
  const limits = new Map(recorded?.programs.map((row) => [row.program, row]));
  const lines = [
    "| Program | Retains | Releases | Uniqueness | Total | Change | No-op calls | Skipped by a test | Change |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
  ];
  for (const row of actual) {
    const limit = limits.get(row.program);
    const delta = limit ? BigInt(row.total) - BigInt(limit.total) : 0n;
    const noOpsDelta =
      limit?.noOpsAndSkipped === undefined
        ? 0n
        : BigInt(noOpsAndSkipped(row)) - BigInt(limit.noOpsAndSkipped);
    lines.push(
      `| ${row.program} | ${row.retained} | ${row.released} | ${row.uniques} | ${row.total} | ${signed(delta)} | ${row.noOps} | ${row.skipped} | ${signed(noOpsDelta)} |`,
    );
  }
  return lines.join("\n") + "\n";
}

function signed(delta: bigint): string {
  return `${delta > 0n ? "+" : ""}${delta}`;
}
