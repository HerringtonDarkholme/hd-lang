// Redundancy and gaps by spec section (audit step 1.6).
// Needs inventory.tsv. Writes coverage.tsv and near-duplicates.tsv.
// Usage: node --experimental-strip-types audit/scripts/test-quality/coverage.ts
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { type FixtureInfo, header, readConformance, readFixtures, root, tsv } from "./lib.ts";

const out = resolve(root, "audit/evidence/01-test-quality");

function anchor(heading: string): string {
  return heading.toLowerCase().replaceAll(/[`,]/g, "").replaceAll(/[^a-z0-9 -]/g, "").trim().replaceAll(/\s+/g, "-");
}

function tokens(fixture: FixtureInfo): Set<string> {
  const text = fixture.lines.filter((line) => !line.startsWith("#")).map((line) => line.replace(/#.*$/, "")).join("\n");
  return new Set(text.match(/[A-Za-z_][A-Za-z0-9_]*|\d+|[^\sA-Za-z0-9_]/g) ?? []);
}

function jaccard(left: Set<string>, right: Set<string>): number {
  let shared = 0;
  for (const token of left) if (right.has(token)) shared += 1;
  return shared / (left.size + right.size - shared || 1);
}

function kind(fixture: FixtureInfo): string {
  return fixture.markers.map((m) => `${m.kind}:${m.code}`).join(",") || "accept";
}

function main(): void {
  const sections: string[] = [];
  for (const file of readdirSync(resolve(root, "spec")).filter((name) => /^\d\d-.*\.md$/.test(name)).sort()) {
    sections.push(file);
    for (const line of readFileSync(resolve(root, "spec", file), "utf8").split("\n"))
      if (/^###? /.test(line)) sections.push(`${file}#${anchor(line.replace(/^#+ /, ""))}`);
  }
  const conformance = readConformance();
  const inventory = readFileSync(resolve(out, "inventory.tsv"), "utf8").trimEnd().split("\n").slice(2).map((row) => row.split("\t"));
  const fixtureSections = new Map<string, number>();
  for (const row of inventory)
    if (row[0] === "test/fixtures" && row[4] === "language behavior") {
      const section = row[5]!.split(" -> ")[0]!;
      fixtureSections.set(section, (fixtureSections.get(section) ?? 0) + 1);
    }
  const rows: unknown[][] = [["section", "conformance_cases", "conformance_selected", "fixture_cases", "status"]];
  for (const section of sections) {
    const cases = conformance.filter((c) => c.section === section);
    const fixtures = fixtureSections.get(section) ?? 0;
    const status = cases.length === 0 && fixtures === 0 ? "no fixture" : cases.length === 0 ? "fixtures only (promotion candidate)" : "";
    rows.push([section, cases.length, cases.filter((c) => c.selected).length, fixtures, status]);
  }
  for (const section of fixtureSections.keys())
    if (!sections.includes(section)) rows.push([section, 0, 0, fixtureSections.get(section), "UNKNOWN ANCHOR"]);
  writeFileSync(resolve(out, "coverage.tsv"), `${header("node --experimental-strip-types audit/scripts/test-quality/coverage.ts")}\n${tsv(rows)}\n`);

  const all = [...conformance, ...readFixtures()];
  const toks = all.map(tokens);
  const pairs: unknown[][] = [["similarity", "left", "right", "expectation"]];
  for (let i = 0; i < all.length; i += 1)
    for (let j = i + 1; j < all.length; j += 1) {
      if (kind(all[i]!) !== kind(all[j]!)) continue;
      const score = jaccard(toks[i]!, toks[j]!);
      if (score >= 0.8)
        pairs.push([score.toFixed(2), `${all[i]!.body}/${all[i]!.path}`, `${all[j]!.body}/${all[j]!.path}`, kind(all[i]!)]);
    }
  writeFileSync(resolve(out, "near-duplicates.tsv"), `${header("node --experimental-strip-types audit/scripts/test-quality/coverage.ts")}\n${tsv(pairs)}\n`);
  const noFixture = rows.filter((row) => row[4] === "no fixture").length;
  const fixturesOnly = rows.filter((row) => String(row[4]).startsWith("fixtures only")).length;
  console.log(`sections=${sections.length} noFixture=${noFixture} fixturesOnly=${fixturesOnly} nearDuplicatePairs=${pairs.length - 1}`);
}

main();
