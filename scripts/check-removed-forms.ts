// Fails when compiler source still has a name of a removed language form.
//
// scripts/removed-forms.tsv lists each form the spec removed, narrowed or
// renamed, with the Rust names that implemented it. A spec change that removes
// a form adds its row (owner, 2026-11-07: "you don't actually improve your code
// against new spec"). A name found as a whole word in crates/ fails the
// check, unless the row names the open ticket that deletes it. A row with a
// ticket whose names are all gone is reported, so the ticket can be closed.
//
// The docs (scripts/doc-files.ts) must describe the current language and
// tree. A row's doc pattern, a regular expression for the removed syntax or
// its diagnostics, fails wherever it matches a doc line, unless that line
// says the form is removed or is a syntax-error. A relative Markdown link, or
// a backticked repository path such as `crates/hd_tir/src/own.rs`, that names
// no file or folder fails too; a line that points into git history is exempt.
// Dated records under docs/audit/ get the link check only.
//
// Usage: node scripts/check-removed-forms.ts

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { HISTORY, docFiles } from "./doc-files.ts";

const root = join(import.meta.dirname, "..");
const rows = readFileSync(join(root, "scripts", "removed-forms.tsv"), "utf8")
  .split("\n")
  .slice(1)
  .filter((line) => line.trim() !== "")
  .map((line) => {
    const [form = "", names = "", ticket = "-", , pattern = "-"] = line.split("\t");
    return {
      form,
      names: names.split(" ").filter(Boolean),
      ticket,
      pattern: pattern === "-" ? undefined : new RegExp(pattern),
    };
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

const docs = docFiles(root);
const LINK = /\]\(([^)\s]+)\)/g;
const PATH =
  /`((?:crates|spec|scripts|docs|lib|website|host|packages|bench|conformance)\/[A-Za-z0-9_./-]*|tests\/[A-Za-z0-9_./-]*\.rs)(?::\d+)?`/g;
const SAYS_REMOVED = /removed|syntax[- ]error/i;
const docFailures: string[] = [];
const exists = (file: string, path: string): boolean =>
  [root, join(root, dirname(file)), join(root, dirname(file), "src")].some((base) =>
    existsSync(join(base, path)),
  );
// The relative Markdown links outside code on one line that name no file.
const deadLinks = (file: string, prose: string): string[] =>
  [...prose.matchAll(LINK)]
    .map(([, target = ""]) => target.split("#")[0] ?? "")
    .filter((path) => path !== "" && !/^[a-z]+:/.test(path))
    .filter((path) => !existsSync(join(root, dirname(file), decodeURI(path))))
    .map((path) => `link to missing ${path}`);

// The missing paths and removed forms a current (not dated) doc line names.
const staleNames = (file: string, line: string): string[] => {
  const found = [...line.matchAll(PATH)]
    .map(([, path = ""]) => path)
    .filter((path) => !exists(file, path))
    .map((path) => `missing path ${path}`);
  if (!SAYS_REMOVED.test(line)) {
    for (const row of rows) {
      if (row.pattern?.test(line)) {
        found.push(`removed form (${row.form}): ${line.trim().slice(0, 100)}`);
      }
    }
  }
  return found;
};

for (const { file, text, dated } of docs) {
  let fenced = false;
  let inCode = false;
  text.split("\n").forEach((line, index) => {
    if (/^\s*```/.test(line)) {
      fenced = !fenced;
    }
    if (fenced || HISTORY.test(line)) {
      return;
    }
    // Inline code may span lines within a paragraph; a link inside it is code.
    const parts = line.split("`");
    const prose = parts.filter((_, i) => (i % 2 === 0) !== inCode).join(" ");
    inCode = line.trim() === "" ? false : (parts.length % 2 === 0) !== inCode;
    const found = deadLinks(file, prose).concat(dated ? [] : staleNames(file, line));
    docFailures.push(...found.map((what) => `${file}:${index + 1}: ${what}`));
  });
}

for (const line of done) {
  console.log(line);
}
if (failures.length > 0) {
  process.stderr.write(`removed forms still in crates/ with no ticket:\n${failures.join("\n")}\n`);
  process.exitCode = 1;
}
if (docFailures.length > 0) {
  process.stderr.write(
    `${docFailures.length} doc line(s) name a removed form or a missing path:\n${docFailures.join("\n")}\n`,
  );
  process.exitCode = 1;
}
