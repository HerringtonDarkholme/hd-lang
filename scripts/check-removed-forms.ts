// Fails when compiler source still has a name of a removed language form.
//
// scripts/removed-forms.tsv lists each form the spec removed, narrowed or
// renamed, with the Rust names that implemented it. A spec change that removes
// a form adds its row (owner, 2026-11-07: "you don't actually improve your code
// against new spec"). A name found as a whole word in crates/ fails the
// check, unless the row names the open ticket that deletes it. A row with a
// ticket whose names are all gone is reported, so the ticket can be closed.
//
// Usage: node scripts/check-removed-forms.ts

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const rows = readFileSync(join(root, "scripts", "removed-forms.tsv"), "utf8")
  .split("\n")
  .slice(1)
  .filter((line) => line.trim() !== "")
  .map((line) => {
    const [form = "", names = "", ticket = "-"] = line.split("\t");
    return { form, names: names.split(" ").filter(Boolean), ticket };
  });

const files = execFileSync("git", ["ls-files", "-z", "--", "crates/*.rs", "crates/**/*.rs"], {
  cwd: root,
  encoding: "utf8",
})
  .split("\0")
  .filter(Boolean)
  .map((file) => ({ file, text: readFileSync(join(root, file), "utf8") }));

const escape = (name: string): string => name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const failures: string[] = [];
const done: string[] = [];
for (const row of rows) {
  let left = 0;
  for (const name of row.names) {
    const word = new RegExp(`(?<![A-Za-z0-9_])${escape(name)}(?![A-Za-z0-9_])`);
    const hits = files.filter(({ text }) => word.test(text)).map(({ file }) => file);
    left += hits.length;
    if (hits.length > 0 && row.ticket === "-") {
      failures.push(`${name} (${row.form}) is back in ${hits.join(", ")}`);
    }
  }
  if (left === 0 && row.ticket !== "-") {
    done.push(`${row.ticket}: every name of "${row.form}" is gone; set its ticket to -`);
  }
}

for (const line of done) {
  console.log(line);
}
if (failures.length > 0) {
  process.stderr.write(`removed forms still in crates/ with no ticket:\n${failures.join("\n")}\n`);
  process.exitCode = 1;
}
