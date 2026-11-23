import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { compareSites, readSiteReport, readSites } from "./conservative-baseline.ts";
import type { SiteReport } from "./conservative-baseline.ts";

const header = "part\tside\treason\taction\tcount\n";
const digest = "bench\tloop.hd\t1234567890abcdef\n";
const report = (entry = 2, call = 3): SiteReport =>
  readSiteReport(
    header +
      `bench\t-\tbuilt\t-\t1\nbench\tapp\tconservative: entry\tretain\t${entry}\nbench\tstd\tconservative: Call\tcopy\t${call}\nbench\truntime\tby design\trelease\t7\n`,
    digest,
    ["loop.hd"],
  );

test("one extra conservative site is an increase even when another reason falls", () => {
  const actual = report(3, 0);
  const comparison = compareSites(actual, report());
  assert.deepEqual(comparison.increases, ["conservative: entry: conservative sites 2 -> 3 (+1)"]);
  assert.deepEqual(comparison.decreases, ["conservative: Call: conservative sites 3 -> 0 (-3)"]);
  assert.match(comparison.table, /3 conservative \/ 10 total sites/);
  // Deliberate named counts are reported but never judged.
  actual.reasons.find((row) => row.reason === "by design")!.sites += 100;
  assert.equal(compareSites(actual, report()).increases.length, 1);
  // Without a base, nothing is judged.
  assert.deepEqual(compareSites(actual).increases, []);
});

test("a new reason starts from zero; an eliminated reason is a decrease", () => {
  const actual = report();
  actual.reasons.push({ reason: "conservative: new glue", app: 1, std: 0, runtime: 0, sites: 1 });
  assert.match(
    compareSites(actual, report()).increases.join(),
    /new glue: conservative sites 0 -> 1/,
  );
  const eliminated = report();
  eliminated.reasons = eliminated.reasons.filter((row) => row.reason !== "conservative: entry");
  assert.match(
    compareSites(eliminated, report()).decreases.join(),
    /entry: conservative sites 2 -> 0/,
  );
});

test("every benchmark must build and report a complete, unambiguous tally", () => {
  const valid = header + "bench\t-\tbuilt\t-\t1\nbench\tapp\tconservative: entry\tretain\t1\n";
  assert.equal(readSiteReport(valid, digest, ["loop.hd"]).reasons[0].sites, 1);
  for (const action of ["retain", "copy", "release", "free", "unique"]) {
    assert.equal(
      readSiteReport(valid.replace("\tretain\t", `\t${action}\t`), digest, ["loop.hd"]).reasons[0]
        .sites,
      1,
    );
  }
  for (const broken of [digest.replace(/1234567890abcdef/, "panic"), digest + digest, ""]) {
    assert.throws(() => readSiteReport(valid, broken, ["loop.hd"]));
  }
  assert.throws(() => readSiteReport(valid, digest, ["loop.hd", "missing.hd"]), /missing/);
  for (const broken of [
    valid + "bench\t-\tskipped\t-\t1\n",
    valid + "bench\tapp\tconservative: entry\tretain\t1\n",
    valid.replace("\tapp\t", "\tother\t"),
    valid.replace("\tretain\t1", "\tretain\t-1"),
    valid.replace("bench\t-\tbuilt\t-\t1\n", ""),
    valid.replace("part\t", "corpus\t"),
  ]) {
    assert.throws(() => readSiteReport(broken, digest, ["loop.hd"]));
  }
});

test("a malformed stored measurement is refused", () => {
  const stored = report();
  assert.deepEqual(readSites(JSON.stringify(stored)), stored);
  for (const reasons of [
    [...stored.reasons, stored.reasons[0]],
    [{ reason: "conservative: entry", sites: -1 }],
    [{ reason: "conservative: entry", sites: Number.MAX_SAFE_INTEGER + 1 }],
    undefined,
  ]) {
    assert.throws(() => readSites(JSON.stringify({ ...stored, reasons })));
  }
});

test("CI command sabotage: one more site than the base exits nonzero without Cost-OK", (context) => {
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const dir = mkdtempSync(resolve(tmpdir(), "hd-conservative-sites-"));
  context.after(() => rmSync(dir, { recursive: true, force: true }));
  const programs = ["bench/study", "bench/runtime/progs"].flatMap((part) =>
    readdirSync(resolve(root, part))
      .filter((name) => name.endsWith(".hd"))
      .map((name) => `${part}/${name}`),
  );
  const rows =
    header + `bench\t-\tbuilt\t-\t${programs.length}\nbench\tapp\tconservative: entry\tretain\t1\n`;
  const digests = programs.map((program) => `bench\t${program}\t1234567890abcdef\n`).join("");
  writeFileSync(resolve(dir, "reasons.tsv"), rows);
  writeFileSync(resolve(dir, "digests.tsv"), digests);
  const base = resolve(dir, "base.json");
  writeFileSync(base, JSON.stringify(readSiteReport(rows, digests, programs)));
  const run = (...extra: string[]) =>
    spawnSync(
      process.execPath,
      [
        resolve(root, "bench/counts/conservative-sites.ts"),
        "--input",
        dir,
        "--output",
        resolve(dir, "out"),
        ...extra,
      ],
      { encoding: "utf8" },
    );
  assert.equal(run("--base", base).status, 0);
  writeFileSync(resolve(dir, "reasons.tsv"), rows.replace("\tretain\t1\n", "\tretain\t2\n"));
  const sabotaged = run("--base", base);
  assert.equal(sabotaged.status, 1);
  assert.match(sabotaged.stderr, /conservative sites 1 -> 2 \(\+1\)/);
  assert.equal(run("--base", base, "--cost-ok", "One new pending path.").status, 0);
  assert.equal(run("--base", resolve(dir, "missing.json")).status, 0);
  assert.equal(run().status, 0);
});
