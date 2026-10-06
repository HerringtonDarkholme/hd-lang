import assert from "node:assert/strict";
import test from "node:test";

import {
  mergedBaseline,
  type Baseline,
  type BaselineCase,
} from "./perf/inference/baseline-merge.ts";

const gate = [{ name: "sample", scales: [10, 100] as const }];

function baseline(score: number, changes: Partial<Baseline> = {}): Baseline {
  const measured: BaselineCase = {
    scales: [10, 100],
    scores: [score, score * 10],
    ratio: score * 2,
    codes: ["type-mismatch"],
  };
  return {
    note: "one run",
    recorded: { date: "2026-10-05", node: "v24.19.0", platform: "linux-x64" },
    referenceMs: score * 100,
    cases: { sample: measured },
    ...changes,
  };
}

test("baseline merge takes the nearest-rank high percentile of each measurement", () => {
  const merged = mergedBaseline(
    [1, 2, 3, 4, 5].map((score) => baseline(score)),
    90,
    gate,
  );
  assert.deepEqual(merged.cases.sample, {
    scales: [10, 100],
    scores: [5, 50],
    ratio: 10,
    codes: ["type-mismatch"],
  });
  assert.equal(merged.referenceMs, 500);
  assert.equal(merged.recorded.runs, 5);
  assert.equal(merged.recorded.percentile, 90);
});

test("baseline merge rejects measurements from unlike runners or cases", () => {
  assert.throws(
    () =>
      mergedBaseline(
        [
          baseline(1),
          baseline(2, { recorded: { date: "2026-10-05", node: "v25", platform: "linux-x64" } }),
        ],
        90,
        gate,
      ),
    /different Node versions/,
  );
  assert.throws(
    () =>
      mergedBaseline(
        [
          baseline(1),
          baseline(2, {
            cases: {
              sample: { ...baseline(2).cases.sample!, codes: ["other"] },
            },
          }),
        ],
        90,
        gate,
      ),
    /different result codes/,
  );
});
