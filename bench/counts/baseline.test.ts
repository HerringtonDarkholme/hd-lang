import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { compareCounts, countEntry, readCounts, verdict } from "./baseline.ts";
import { readResult, resultKey, writeResult } from "./results.ts";

test("each program is judged on its own total, with exact full-width counters", () => {
  const base = countEntry("hot-loop", [1n << 60n, 4n, 2n]);
  const head = countEntry("hot-loop", [(1n << 60n) + 1000n, 1004n, 2n]);
  const cheaper = countEntry("other", [1n, 1n, 0n]);
  const comparison = compareCounts([head, cheaper], [base, countEntry("other", [5n, 5n, 0n])]);
  assert.equal(head.total, "1152921504606848982");
  assert.deepEqual(comparison.increases, [
    "hot-loop: count operations 1152921504606846982 -> 1152921504606848982 (+2000)",
  ]);
  assert.deepEqual(comparison.decreases, ["other: count operations 10 -> 2 (-8)"]);
  assert.match(comparison.table, /\| \+2000 \|/);
  assert.equal(countEntry("unsigned", [-1n, 0n, 0n]).retained, "18446744073709551615");
  assert.deepEqual(compareCounts([base], [base]).increases, []);
});

test("no-op and skipped calls are judged as one sum beside the total", () => {
  const base = countEntry("loop", [5n, 5n, 0n], 100n, 20n);
  // The two move in opposite directions: only the sum counts.
  assert.deepEqual(
    compareCounts([countEntry("loop", [5n, 5n, 0n], 90n, 30n)], [base]).increases,
    [],
  );
  for (const raised of [
    countEntry("loop", [5n, 5n, 0n], 101n, 20n),
    countEntry("loop", [1n, 1n, 0n], 100n, 21n),
  ]) {
    assert.match(
      compareCounts([raised], [base]).increases.join("\n"),
      /no-op and skipped calls 120 -> 121/,
    );
  }
});

test("an increase fails unless the change says Cost-OK; no base only reports", () => {
  const base = [countEntry("loop", [1n, 1n, 0n])];
  const up = compareCounts([countEntry("loop", [2n, 1n, 0n])], base);
  assert.equal(verdict(up, true).failed, true);
  assert.match(verdict(up, true).text, /Cost-OK: <reason>.*\n\n- loop: count operations 2 -> 3/);
  const accepted = verdict(up, true, "The loop returns an owned result.");
  assert.equal(accepted.failed, false);
  assert.match(accepted.text, /accepted by `Cost-OK: The loop returns/);
  assert.equal(verdict(up, true, "  ").failed, true);
  assert.equal(verdict(compareCounts([countEntry("loop", [2n, 1n, 0n])]), false).failed, false);
  const down = verdict(compareCounts([countEntry("loop", [0n, 1n, 0n])], base), true);
  assert.equal(down.failed, false);
  assert.match(down.text, /Decreases from the base:\n\n- loop: count operations 2 -> 1 \(-1\)/);
  // A benchmark the base lacks is new, not an increase.
  const added = compareCounts([countEntry("new", [9n, 9n, 0n])], base);
  assert.deepEqual(added.increases, []);
  assert.match(added.table, /\| new \|/);
});

test("a stored measurement that does not add up is refused", () => {
  const row = countEntry("loop", [1n, 1n, 0n], 3n, 4n);
  assert.deepEqual(readCounts(JSON.stringify({ programs: [row] })), [row]);
  for (const programs of [
    [row, row],
    [{ ...row, total: "1" }],
    [{ ...row, retained: "-1" }],
    [{ ...row, noOps: undefined }],
    undefined,
  ]) {
    assert.throws(() => readCounts(JSON.stringify({ programs })));
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
