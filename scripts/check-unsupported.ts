// `unsupported` is banned (owner, 2026-11-08: "ban fucking unsupported shit").
//
// A compiler that refuses a program the spec accepts is a gap, and a refusal
// of a case an earlier stage rules out is dead code. Neither may be added.
// This check counts the sites that remain in each crate's non-test source
// (calls of `unsupported(` and constructions of `Code::Unsupported`) and
// fails when a crate has more than its ceiling in scripts/unsupported-ceiling.tsv.
// When a crate has fewer, it prints the new count so the ceiling is lowered in
// the same change: the ceilings only go down, until `Code::Unsupported` itself
// is deleted (#1489, #1490).
//
// Usage: node scripts/check-unsupported.ts [--update]
//
// Cost per run: one `git ls-files` and one read of each Rust source file under
// crates/*/src (cost: bounded by the source tree, the same files a build reads).

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const ceilingPath = join(root, "scripts", "unsupported-ceiling.tsv");
const site = /\bunsupported\(|Code::Unsupported\b/g;

/** The non-test part of a Rust file: everything before its first `#[cfg(test)]`. */
function nonTest(text: string): string {
  const at = text.indexOf("#[cfg(test)]");
  return at < 0 ? text : text.slice(0, at);
}

function countSites(): Map<string, number> {
  // cost: one listing of tracked files, read once per run.
  const files = execFileSync(
    "git",
    ["ls-files", "-z", "--", "crates/*/src/*.rs", "crates/*/src/**/*.rs"],
    {
      cwd: root,
      encoding: "utf8",
    },
  )
    .split("\0")
    .filter(Boolean);
  const counts = new Map<string, number>();
  for (const file of files) {
    const crate = file.split("/")[1]!;
    const body = nonTest(readFileSync(join(root, file), "utf8"))
      .split("\n")
      .filter((line) => !line.trimStart().startsWith("//"))
      .join("\n");
    const found = body.match(site)?.length ?? 0;
    if (found > 0) {
      counts.set(crate, (counts.get(crate) ?? 0) + found);
    }
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
