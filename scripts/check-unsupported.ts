// `unsupported` is banned (owner, 2026-11-08: "ban fucking unsupported shit").
//
// A compiler that refuses a program the spec accepts is a gap, and a refusal
// of a case an earlier stage rules out is dead code. Neither may be added.
// This check counts the matches of the ast-grep rule `ban-unsupported`
// (.ast-grep/rules/ban-unsupported.yml: calls of `unsupported(` and
// constructions of `Code::Unsupported` outside test code) per crate, and
// fails when a crate has more than its ceiling in scripts/unsupported-ceiling.tsv.
// When a crate has fewer, it prints the new count so the ceiling is lowered in
// the same change: the ceilings only go down, until `Code::Unsupported` itself
// is deleted (#1489, #1490).
//
// Usage: node scripts/check-unsupported.ts [--update]
//
// Cost per run: one ast-grep scan, the same one check:cost runs.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const ceilingPath = join(root, "scripts", "unsupported-ceiling.tsv");

/** Matches of the ast-grep rule `ban-unsupported` (.ast-grep/rules/), by crate. */
function countSites(): Map<string, number> {
  // cost: one ast-grep scan over the repository, filtered to this rule.
  const scan = spawnSync(
    "ast-grep",
    ["scan", "--config", ".ast-grep/sgconfig.yml", "--filter", "ban-unsupported", "--json=compact"],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (scan.error) {
    throw new Error(`check-unsupported needs ast-grep on PATH: ${scan.error.message}`);
  }
  if (scan.status !== 0 && scan.status !== 1) {
    throw new Error(`ast-grep scan failed (status ${scan.status}):\n${scan.stderr}`);
  }
  const counts = new Map<string, number>();
  for (const match of JSON.parse(scan.stdout) as { file: string }[]) {
    const crate = match.file.split("/")[1]!;
    counts.set(crate, (counts.get(crate) ?? 0) + 1);
  }
  return counts;
}

function readCeilings(): Map<string, number> {
  const ceilings = new Map<string, number>();
  for (const line of readFileSync(ceilingPath, "utf8").split("\n").slice(1)) {
    const [crate, count] = line.split("\t");
    if (crate && count) {
      ceilings.set(crate, Number(count));
    }
  }
  return ceilings;
}

function write(counts: Map<string, number>): void {
  const rows = [...counts]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([crate, count]) => `${crate}\t${count}`);
  writeFileSync(ceilingPath, ["crate\tceiling", ...rows].join("\n") + "\n");
}

const counts = countSites();
if (process.argv.includes("--update")) {
  write(counts);
  console.log(`wrote ${ceilingPath}`);
} else {
  const ceilings = readCeilings();
  const failures: string[] = [];
  const lower: string[] = [];
  for (const crate of new Set([...counts.keys(), ...ceilings.keys()])) {
    const now = counts.get(crate) ?? 0;
    const ceiling = ceilings.get(crate) ?? 0;
    if (now > ceiling) {
      failures.push(
        `${crate}: ${now} unsupported sites, ceiling ${ceiling}. unsupported is banned: implement the case, or reject the program with a real rule and ticket the gap.`,
      );
    } else if (now < ceiling) {
      lower.push(`${crate}: ${now} < ceiling ${ceiling}`);
    }
  }
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exitCode = 1;
  } else if (lower.length > 0) {
    console.error(
      `ceilings above the count; lower them with --update in this change:\n${lower.join("\n")}`,
    );
    process.exitCode = 1;
  }
}
