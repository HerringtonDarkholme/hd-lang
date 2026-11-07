import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { compareCounts, countEntry, countTable, readBaseline, recordCounts } from "./baseline.ts";
import { readResult, resultKey, writeResult } from "./results.ts";

test("each program blocks on its own total, with exact full-width counters", () => {
  const base = countEntry("hot-loop", [1n << 60n, 4n, 2n]);
  const head = countEntry("hot-loop", [(1n << 60n) + 1000n, 1004n, 2n]);
  const baseline = recordCounts([base]);
  assert.equal(compareCounts([head], baseline).length, 1);
  assert.equal(head.total, "1152921504606848982");
  assert.match(countTable([head], baseline), /\+2000/);
  assert.equal(countEntry("unsigned", [-1n, 0n, 0n]).retained, "18446744073709551615");
  assert.deepEqual(readBaseline(JSON.stringify(baseline)), baseline);
  assert.deepEqual(compareCounts([base], baseline), []);
});
test("raising a limit needs a fresh checked-in reason and the measured total", () => {
  const base = countEntry("hot-loop", [1n, 1n, 0n]);
  const head = countEntry("hot-loop", [1001n, 1001n, 0n]);
  const before = recordCounts([base]);
  assert.throws(() => recordCounts([head], before), /--reason/);
  const explained = recordCounts(
    [head],
    before,
    "The loop now returns an additional owned result.",
  );
  assert.deepEqual(compareCounts([head], explained, before), []);
  assert.match(compareCounts([head], recordCounts([head]), before).join("\n"), /explanation/);
  const increasedAgain = recordCounts(
    [countEntry("hot-loop", [1002n, 1002n, 0n])],
    explained,
    explained.programs[0].reason,
  );
  assert.match(
    compareCounts([countEntry("hot-loop", [1002n, 1002n, 0n])], increasedAgain, explained).join(
      "\n",
    ),
    /new explanation/,
  );
  const inflated = recordCounts([countEntry("hot-loop", [9999n, 9999n, 0n])], before, "increased");
  assert.match(compareCounts([head], inflated, before).join("\n"), /must equal/);
  assert.match(compareCounts([head], explained, explained).join("\n"), /^$/);
});
test("each program blocks on its no-op and skipped calls, with total unchanged", () => {
  const base = countEntry("loop", [5n, 5n, 0n], 100n, 20n);
  const before = recordCounts([base]);
  assert.equal(before.programs[0].noOpsAndSkipped, "120");
  assert.deepEqual(compareCounts([base], before), []);
  // The two move in opposite directions: only the sum is limited.
  assert.deepEqual(compareCounts([countEntry("loop", [5n, 5n, 0n], 90n, 30n)], before), []);
  for (const raised of [
    countEntry("loop", [5n, 5n, 0n], 101n, 20n),
    countEntry("loop", [5n, 5n, 0n], 100n, 21n),
  ]) {
    assert.match(compareCounts([raised], before).join("\n"), /no-op and skipped calls 120 -> 121/);
    assert.throws(() => recordCounts([raised], before), /--reason/);
    const explained = recordCounts([raised], before, "A new count site that finds no count.");
    assert.deepEqual(compareCounts([raised], explained, before), []);
    assert.match(compareCounts([raised], recordCounts([raised]), before).join("\n"), /explanation/);
  }
  // A limit above what was measured can't make room for a later rise.
  const inflated = recordCounts([countEntry("loop", [5n, 5n, 0n], 900n, 0n)], before, "increased");
  assert.match(compareCounts([base], inflated, before).join("\n"), /must equal/);
  // Lower sums refresh without a reason, and a reason for the old sum goes with it.
  const explained = recordCounts([countEntry("loop", [5n, 5n, 0n], 200n, 0n)], before, "why");
  const lower = recordCounts([base], explained);
  assert.equal(lower.programs[0].noOpsAndSkipped, "120");
  assert.equal(lower.programs[0].reason, undefined);
  // A raised sum beside a lower total still needs the reason.
  const cheaper = countEntry("loop", [1n, 1n, 0n], 121n, 0n);
  assert.throws(() => recordCounts([cheaper], before), /--reason/);
});
test("a baseline older than the no-op limit seeds it, and can't skip the check", () => {
  const base = countEntry("loop", [5n, 5n, 0n], 100n, 20n);
  const older = recordCounts([base], undefined);
  const { noOpsAndSkipped: _limit, ...row } = older.programs[0];
  const unlimited = { ...older, programs: [{ ...row, reason: "kept" }] };
  assert.deepEqual(readBaseline(JSON.stringify(unlimited)), unlimited);
  assert.match(compareCounts([base], unlimited).join("\n"), /no limit for no-op/);
  const seeded = recordCounts([base], unlimited);
  assert.equal(seeded.programs[0].reason, "kept");
  assert.deepEqual(compareCounts([base], seeded, unlimited), []);
});
test("lower totals refresh without a waiver and program sets stay exact", () => {
  const base = countEntry("loop", [10n, 10n, 1n]);
  const head = countEntry("loop", [9n, 9n, 1n]);
  const before = recordCounts([base]);
  assert.deepEqual(compareCounts([head], before), []);
  assert.equal(recordCounts([head], before).programs[0].total, "19");
  assert.match(compareCounts([], before).join("\n"), /no measured/);
  assert.match(
    compareCounts([countEntry("new", [0n, 0n, 0n])], before).join("\n"),
    /missing baseline/,
  );
});
test("malformed and duplicate baselines cannot waive a regression", () => {
  const row = countEntry("loop", [1n, 1n, 0n]);
  const base = recordCounts([row]);
  for (const programs of [
    [row, row],
    [{ ...row, total: "1" }],
    [{ ...row, retained: "-1" }],
    [{ ...base.programs[0], noOpsAndSkipped: "many" }],
  ]) {
    assert.throws(() => readBaseline(JSON.stringify({ ...base, programs })));
  }
});
test("a stored result answers only for the same bytes, program and runner", () => {
  const bytes = new Uint8Array([0, 97, 115, 109]);
  const key = resultKey("runner", "loop", bytes);
  assert.notEqual(key, resultKey("runner", "loop", new Uint8Array([0, 97, 115, 110])));
  assert.notEqual(key, resultKey("runner", "other", bytes));
  assert.notEqual(key, resultKey("other", "loop", bytes));
  const directory = mkdtempSync(join(tmpdir(), "count-results-"));
  try {
    assert.equal(readResult(directory, key, "loop"), undefined);
    const row = countEntry("loop", [1n << 60n, 4n, 2n], 7n, 3n);
    writeResult(directory, key, row);
    assert.deepEqual(readResult(directory, key, "loop"), row);
    assert.equal(readResult(directory, key, "other"), undefined);
    for (const bad of [
      "{",
      JSON.stringify({ ...row, total: "1" }),
      JSON.stringify({ ...row, noOps: undefined }),
      JSON.stringify({ ...row, skipped: undefined }),
    ]) {
      writeFileSync(join(directory, `${key}.json`), bad);
      assert.equal(readResult(directory, key, "loop"), undefined);
    }
  } finally {
    rmSync(directory, { recursive: true });
  }
});
