import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { mapUncovered, newUncovered, previousMeasurement, type Range } from "./history.ts";

const range = (first: number, last = first, file = "a.rs"): Range => ({ file, first, last });
const diff = (hunks: string) => `diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n${hunks}\n`;

test("insertions, deletions and replacements map only unchanged old gaps", () => {
  const before = [range(1, 10)];
  assert.deepEqual(mapUncovered(before, diff("@@ -0,0 +1,2 @@")), [range(3, 12)]);
  assert.deepEqual(mapUncovered(before, diff("@@ -3,2 +2,0 @@")), [range(1, 2), range(3, 8)]);
  assert.deepEqual(mapUncovered(before, diff("@@ -3,2 +3,3 @@")), [range(1, 2), range(6, 11)]);
  assert.deepEqual(mapUncovered(before, diff("@@ -10,0 +11 @@")), [range(1, 10)]);
});

test("multiple edits accumulate offsets across separated gaps", () => {
  const mapped = mapUncovered(
    [range(2), range(5, 7), range(10)],
    diff("@@ -1,0 +2,2 @@\n@@ -6,1 +7,0 @@"),
  );
  assert.deepEqual(mapped, [range(4), range(7), range(8), range(11)]);
});

test("renames preserve gaps, edits within renames do not", () => {
  const header = "diff --git a/a.rs b/b.rs\nrename from a.rs\nrename to b.rs\n";
  assert.deepEqual(mapUncovered([range(4)], header), [range(4, 4, "b.rs")]);
  assert.deepEqual(mapUncovered([range(4)], header + "--- a/a.rs\n+++ b/b.rs\n@@ -1 +1,2 @@\n"), [
    range(5, 5, "b.rs"),
  ]);
  assert.deepEqual(
    mapUncovered(
      [range(4)],
      "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ /dev/null\n@@ -1,5 +0,0 @@\n",
    ),
    [],
  );
});

test("new gaps fail regardless of aggregate percentage or whether source changed", () => {
  assert.deepEqual(newUncovered([range(1, 8)], [range(2, 3), range(6, 7)]), [
    range(1),
    range(4, 5),
    range(8),
  ]);
  assert.deepEqual(newUncovered([range(2), range(8)], [range(1, 10)]), []);
  assert.deepEqual(newUncovered([range(4)], []), [range(4)]);
  assert.deepEqual(newUncovered([], [range(4)]), []);
});

test("45313663181: deleting 56 covered lines preserves all 39 hd_layout gaps", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("fixtures/45313663181.json", import.meta.url), "utf8"),
  );
  const parse = (text: string): Range[] =>
    text
      .trim()
      .split("\n")
      .map((line) => {
        const [file, span] = line.split(":");
        const [first, last = first] = span.split("-").map(Number);
        return { file, first, last };
      });
  const old = parse(fixture.previous);
  const current = parse(fixture.current);
  assert.equal(
    current.reduce((sum, row) => sum + row.last - row.first + 1, 0),
    39,
  );
  assert.deepEqual(newUncovered(current, mapUncovered(old, fixture.diffHeaders)), []);
  const lost = { ...current[0], first: current[0].first + 1, last: current[0].last + 1 };
  assert.deepEqual(newUncovered([...current, lost], mapUncovered(old, fixture.diffHeaders)), [
    lost,
  ]);
});

test("large intervals and many hunks use interval work, independent of line counts", () => {
  const width = 1000;
  const hunks = Array.from({ length: width }, (_, i) => `@@ -${2 * i + 1},0 +${3 * i + 2} @@`).join(
    "\n",
  );
  const mapped = mapUncovered([range(1, 1_000_000_000)], diff(hunks));
  assert.equal(mapped.length, width + 1);
  assert.equal(mapped.at(-1)?.last, 1_000_001_000);
  assert.equal(newUncovered(mapped, mapped).length, 0);
});

test("offline CLI passes covered deletions and fails a newly uncovered unchanged line", () => {
  const dir = mkdtempSync(join(tmpdir(), "hd-coverage-history-"));
  const root = join(import.meta.dirname, "../..");
  const commit = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: root,
    encoding: "utf8",
    timeout: 10_000,
  }).trim();
  const crates = [
    "hd_syntax",
    "hd_resolve",
    "hd_types",
    "hd_check",
    "hd_tir",
    "hd_mono",
    "hd_opt",
    "hd_layout",
    "hd_wasm",
  ];
  const store = (name: string, extra: boolean, covered: number) => {
    const folder = join(dir, name);
    mkdirSync(join(folder, "uncovered"), { recursive: true });
    const rows = Object.fromEntries(
      crates.map((crate) => [
        crate,
        {
          covered,
          total: covered + 1,
          percent: (covered / (covered + 1)) * 100,
          invariant: { covered: 0, total: 0 },
        },
      ]),
    );
    writeFileSync(
      join(folder, "report.json"),
      JSON.stringify({ commit, crates: rows, runs: ["core"], seconds: {}, conformanceOnly: null }),
    );
    for (const crate of crates) {
      writeFileSync(
        join(folder, "uncovered", `${crate}.txt`),
        crate === "hd_layout" ? `crates/hd_layout/src/lib.rs:10${extra ? "-11" : ""}\n` : "",
      );
    }
    return join(folder, "report.json");
  };
  try {
    const base = store("previous", false, 1000);
    const run = (current: string) =>
      spawnSync(
        process.execPath,
        ["scripts/coverage.ts", "--check", "--from", current, "--base", base],
        { cwd: root, encoding: "utf8", timeout: 10_000 },
      );
    assert.equal(run(store("covered-deletion", false, 100)).status, 0);
    const loss = run(store("new-loss", true, 2000));
    assert.equal(loss.status, 1, loss.stdout + loss.stderr);
    assert.match(loss.stdout, /new or newly uncovered/);
    assert.match(loss.stdout, /lib.rs:11/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the latest uploaded measurement becomes the reference even after a failed check", () => {
  const runs = [
    { id: 4, head_sha: "current" },
    { id: 3, head_sha: "failed" },
    { id: 2, head_sha: "successful" },
  ];
  const lookedUp: string[] = [];
  const selected = previousMeasurement(
    runs,
    4,
    () => true,
    (run) => {
      lookedUp.push(run.head_sha);
      return run.head_sha;
    },
  );
  assert.equal(selected, "failed");
  assert.deepEqual(lookedUp, ["failed"]);
  // Yesterday's failure is today's baseline: unchanged gaps recover to green.
  const failed = [range(10), range(20)];
  assert.deepEqual(newUncovered(failed, failed), []);
  assert.deepEqual(newUncovered([...failed, range(30)], failed), [range(30)]);
});

test("reference selection skips non-ancestors and unavailable or incomplete measurements", () => {
  const runs = ["other-branch", "missing", "expired", "partial", "measured", "older"].map(
    (head_sha, i) => ({ id: 10 - i, head_sha }),
  );
  const ancestry: string[] = [];
  const lookedUp: string[] = [];
  const selected = previousMeasurement(
    runs,
    Infinity,
    (sha) => {
      ancestry.push(sha);
      return sha !== "other-branch";
    },
    (run) => {
      lookedUp.push(run.head_sha);
      return run.head_sha === "measured" ? run.head_sha : undefined;
    },
  );
  assert.equal(selected, "measured");
  assert.deepEqual(ancestry, ["other-branch", "missing", "expired", "partial", "measured"]);
  assert.deepEqual(lookedUp, ["missing", "expired", "partial", "measured"]);
  assert.equal(
    previousMeasurement(
      [],
      Infinity,
      () => true,
      () => "unused",
    ),
    undefined,
  );
  assert.equal(
    previousMeasurement(
      runs,
      0,
      () => assert.fail("current or newer run"),
      () => "unused",
    ),
    undefined,
  );
});

test("finding a recent measurement does not scan growing older history", () => {
  for (const width of [100, 1000]) {
    const runs = Array.from({ length: width }, (_, i) => ({ id: width - i, head_sha: String(i) }));
    let ancestry = 0;
    let downloads = 0;
    assert.equal(
      previousMeasurement(
        runs,
        Infinity,
        () => {
          ancestry++;
          return true;
        },
        () => {
          downloads++;
          return "measurement";
        },
      ),
      "measurement",
    );
    assert.equal(ancestry, 1);
    assert.equal(downloads, 1);
  }
});
