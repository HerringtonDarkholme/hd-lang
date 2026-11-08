// Sloppy code only goes down (owner, 2026-11-08: "ban fucking unsupported
// shit", then "unsupported is just one kind of sloppy code").
//
// Each kind is an ast-grep rule in .ast-grep/rules/ (`ban-unsupported` and the
// `sloppy-*` rules: `.expect`, `.unwrap`, `unreachable!`, `panic!`, empty
// catch-all arms, hard-coded "std.*" paths, "for now"/workaround comments),
// matched outside test code. This check counts the matches per rule and crate
// and fails when a count exceeds its ceiling in scripts/sloppy-ceiling.tsv.
// A count that falls passes and is reported; the merge script then lowers the
// ceiling (--update) in the same batch, so the ceilings only go down and a
// change that removes sloppy code is never red for it. `unsupported` is banned outright: its ceilings
// reach zero when `Code::Unsupported` is deleted (#1489, #1490).
//
// Usage: node scripts/check-sloppy.ts [--update]
//
// Cost per run: one ast-grep scan over the repository, filtered to these rules.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const ceilingPath = join(root, "scripts", "sloppy-ceiling.tsv");
const filter = "^(ban-unsupported|sloppy-.*)$";

function countSites(): Map<string, number> {
  // cost: one ast-grep scan over the repository, filtered to these rules.
  const scan = spawnSync(
    "ast-grep",
    ["scan", "--config", ".ast-grep/sgconfig.yml", "--filter", filter, "--json=compact"],
    { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  if (scan.error) {
    throw new Error(`check-sloppy needs ast-grep on PATH: ${scan.error.message}`);
  }
  if (scan.status !== 0 && scan.status !== 1) {
    throw new Error(`ast-grep scan failed (status ${scan.status}):\n${scan.stderr}`);
  }
  const counts = new Map<string, number>();
  for (const match of JSON.parse(scan.stdout) as { ruleId: string; file: string }[]) {
    const key = `${match.ruleId}\t${match.file.split("/")[1]!}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

function readCeilings(): Map<string, number> {
  const ceilings = new Map<string, number>();
  for (const line of readFileSync(ceilingPath, "utf8").split("\n").slice(1)) {
    const [rule, crate, count] = line.split("\t");
    if (rule && crate && count) {
      ceilings.set(`${rule}\t${crate}`, Number(count));
    }
  }
  return ceilings;
}

function write(counts: Map<string, number>): void {
  const rows = [...counts]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, count]) => `${key}\t${count}`);
  writeFileSync(ceilingPath, ["rule\tcrate\tceiling", ...rows].join("\n") + "\n");
}

const counts = countSites();
if (process.argv.includes("--update")) {
  const ceilings = readCeilings();
  const raised = [...counts].filter(([key, count]) => count > (ceilings.get(key) ?? 0));
  if (ceilings.size > 0 && raised.length > 0 && !process.argv.includes("--initial")) {
    console.error(
      `--update never raises a ceiling:\n${raised.map(([key, count]) => `${key.replace("\t", " ")}: ${count}`).join("\n")}`,
    );
    process.exitCode = 1;
  } else {
    write(counts);
    console.log(`wrote ${ceilingPath}`);
  }
} else {
  const ceilings = readCeilings();
  const failures: string[] = [];
  const lower: string[] = [];
  for (const key of new Set([...counts.keys(), ...ceilings.keys()])) {
    const now = counts.get(key) ?? 0;
    const ceiling = ceilings.get(key) ?? 0;
    const [rule, crate] = key.split("\t");
    if (now > ceiling) {
      failures.push(
        `${crate}: ${now} ${rule} sites, ceiling ${ceiling}. Sloppy code only goes down: handle the case instead.`,
      );
    } else if (now < ceiling) {
      lower.push(`${crate}: ${rule} ${now} < ceiling ${ceiling}`);
    }
  }
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exitCode = 1;
  } else if (lower.length > 0) {
    console.log(`counts below their ceilings (the merge lowers them):\n${lower.join("\n")}`);
  }
}
