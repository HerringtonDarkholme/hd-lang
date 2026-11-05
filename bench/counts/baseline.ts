export interface Counts {
  retained: string;
  released: string;
  uniques: string;
  total: string;
}
export interface Entry extends Counts {
  program: string;
  reason?: string;
}
export interface Baseline {
  version: 1;
  pipeline: "optimized-release-checked";
  engine: "node-default";
  programs: Entry[];
}

export function countEntry(program: string, values: readonly bigint[]): Entry {
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
  };
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
      (row.reason !== undefined && typeof row.reason !== "string")
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
    if (old && BigInt(row.total) <= BigInt(old.total)) {
      continue;
    }
    if (!row.reason?.trim() || row.reason === old?.reason) {
      failures.push(
        `${row.program}: a new/increased baseline needs a new explanation in its reason field`,
      );
    }
    if (measured.get(row.program)?.total !== row.total) {
      failures.push(`${row.program}: an increased baseline must equal the measured total`);
    }
  }
  return failures;
}

export function recordCounts(actual: Entry[], previous?: Baseline, reason?: string): Baseline {
  const before = new Map(previous?.programs.map((row) => [row.program, row]));
  const programs = actual.map((row) => {
    const old = before.get(row.program);
    const increased = previous && (!old || BigInt(row.total) > BigInt(old.total));
    if (increased && !reason?.trim()) {
      throw new Error(`${row.program}: --reason is required for an increase or a new benchmark`);
    }
    let note = old?.total === row.total ? old.reason : undefined;
    if (increased) {
      note = reason;
    }
    return { ...row, ...(note ? { reason: note } : {}) };
  });
  return { version: 1, pipeline: "optimized-release-checked", engine: "node-default", programs };
}

export function countTable(actual: readonly Entry[], recorded?: Baseline): string {
  const limits = new Map(recorded?.programs.map((row) => [row.program, row]));
  const lines = [
    "| Program | Retains | Releases | Uniqueness | Total | Change |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
  ];
  for (const row of actual) {
    const limit = limits.get(row.program);
    const delta = limit ? BigInt(row.total) - BigInt(limit.total) : 0n;
    lines.push(
      `| ${row.program} | ${row.retained} | ${row.released} | ${row.uniques} | ${row.total} | ${delta > 0n ? "+" : ""}${delta} |`,
    );
  }
  return lines.join("\n") + "\n";
}
