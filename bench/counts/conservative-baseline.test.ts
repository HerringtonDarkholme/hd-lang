import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  compareSites,
  readSiteBudget,
  readSiteReport,
  recordSites,
  siteTable,
} from "./conservative-baseline.ts";
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

test("one extra conservative site fails even when another reason falls", () => {
  const before = recordSites(report());
  const actual = report(3, 0);
  assert.match(compareSites(actual, before).join("\n"), /conservative: entry.*2 -> 3/);
  assert.deepEqual(compareSites(report(1, 2), before), []);
  assert.match(siteTable(actual, before), /3 conservative \/ 10 total sites/);
  // Deliberate named counts are reported but do not spend a conservative budget.
  actual.reasons.find((row) => row.reason === "by design")!.sites += 100;
  assert.equal(compareSites(actual, before).length, 1);
});

test("raising a reason budget needs a fresh explanation and exact measurement", () => {
  const before = recordSites(report());
  const actual = report(3);
  assert.throws(() => recordSites(actual, before), /--reason/);
  const raised = recordSites(actual, before, "The new pending path keeps one argument.");
  assert.deepEqual(compareSites(actual, raised, before), []);
  const again = recordSites(
    report(4),
    raised,
    raised.reasons.find((row) => row.reason.endsWith("entry"))!.explanation,
  );
  assert.match(compareSites(report(4), again, raised).join("\n"), /fresh explanation/);
  const inflated = recordSites(report(99), before, "New pending path.");
  assert.match(compareSites(actual, inflated, before).join("\n"), /must equal/);
  assert.match(compareSites(actual, recordSites(actual), before).join("\n"), /fresh explanation/);
});

test("new reasons cannot borrow another reason's budget; eliminated reasons stay zero", () => {
  const before = recordSites(report());
  const actual = report();
  actual.reasons.push({ reason: "conservative: new glue", app: 1, std: 0, runtime: 0, sites: 1 });
  assert.match(compareSites(actual, before).join("\n"), /new glue.*0 -> 1/);
  assert.throws(() => recordSites(actual, before), /--reason/);
  const eliminated = report();
  eliminated.reasons = eliminated.reasons.filter((row) => row.reason !== "conservative: entry");
  const lower = recordSites(eliminated, before);
  assert.equal(lower.reasons.find((row) => row.reason.endsWith("entry"))!.sites, 0);
  assert.match(compareSites(report(1), lower).join("\n"), /entry.*0 -> 1/);
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

test("invalid and duplicate budgets cannot waive a regression", () => {
  const budget = recordSites(report());
  assert.deepEqual(readSiteBudget(JSON.stringify(budget)), budget);
  for (const reasons of [
    [...budget.reasons, budget.reasons[0]],
    [{ reason: "stored", sites: 100 }],
    [{ reason: "conservative: entry", sites: -1 }],
    [{ reason: "conservative: entry", sites: Number.MAX_SAFE_INTEGER + 1 }],
  ]) {
    assert.throws(() => readSiteBudget(JSON.stringify({ ...budget, reasons })));
  }
});

test("CI command sabotage: adding one reported conservative site exits nonzero", (context) => {
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
  const budget = resolve(dir, "budget.json");
  writeFileSync(budget, JSON.stringify(recordSites(readSiteReport(rows, digests, programs))));
  const run = () =>
    spawnSync(
      process.execPath,
      [
        resolve(root, "bench/counts/conservative-sites.ts"),
        "--input",
        dir,
        "--output",
        resolve(dir, "out"),
        "--baseline",
        budget,
      ],
      { encoding: "utf8" },
    );
  assert.equal(run().status, 0);
  writeFileSync(resolve(dir, "reasons.tsv"), rows.replace("\tretain\t1\n", "\tretain\t2\n"));
  const sabotaged = run();
  assert.equal(sabotaged.status, 1);
  assert.match(sabotaged.stderr, /conservative sites 1 -> 2/);
});
