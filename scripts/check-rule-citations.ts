// Fails when compiler source or a doc cites a spec rule the spec no longer has.
//
// A citation is a backticked rule ID in a Rust file under crates/ or in a doc
// (scripts/doc-files.ts; dated records under docs/audit/ are exempt), such as
// `types.path.access`, whose first segment is a chapter prefix
// (spec/tools/spec-prose.ts CHAPTER_PREFIXES). It holds when the spec has
// that rule, or when it names a group of rules: a prefix that some rule ID
// extends with a dot; a trailing `.*` names such a group. Anything else is stale: the rule was renamed or
// deleted, and the code that cites it still describes the old form
// (owner, 2026-11-07: "you don't actually improve your code against new spec").
//
// Usage: node scripts/check-rule-citations.ts

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { REPO_ROOT, allRules, loadCorpus } from "../spec/tools/spec-corpus.ts";
import { CHAPTER_PREFIXES, RULE_ID } from "../spec/tools/spec-prose.ts";
import { docFiles } from "./doc-files.ts";

const ids = new Set(allRules(loadCorpus()).map(({ rule }) => rule.id));
const groups = new Set<string>();
for (const id of ids) {
  const parts = id.split(".");
  for (let length = 1; length < parts.length; length += 1) {
    groups.add(parts.slice(0, length).join("."));
  }
}
const prefixes = new Set(Object.values(CHAPTER_PREFIXES));
// hd source in comments that has a rule ID's shape: a module path such as
// `module.name` is syntax, not a citation.
const SYNTAX = new Set(["module.item", "module.name", "module.name.member", "module.Type.Variant"]);
// Wasm instructions that docs name: `ref.func` is an opcode, not a rule.
const WASM = new Set(["ref.func", "ref.cast", "ref.null", "ref.eq", "ref.test", "ref.is_null"]);
// A file name such as `expr.rs` has a rule ID's shape too.
const FILE = /\.(?:rs|ts|md|hd|tsv|json|wat)$/;

const files = execFileSync("git", ["ls-files", "-z", "--", "crates/*.rs", "crates/**/*.rs"], {
  cwd: REPO_ROOT,
  encoding: "utf8",
})
  .split("\0")
  .filter(Boolean)
  .map((file) => ({ file, text: readFileSync(join(REPO_ROOT, file), "utf8") }))
  .concat(docFiles(REPO_ROOT).filter(({ dated }) => !dated));

const backticked = /`([^`\s]+)`/g;
const stale: string[] = [];
for (const { file, text } of files) {
  text.split("\n").forEach((line, index) => {
    for (const [, token] of line.matchAll(backticked)) {
      const id = (token ?? "").replace(/\.\*$/, "");
      if (
        !RULE_ID.test(id) ||
        !prefixes.has(id.split(".")[0] ?? "") ||
        SYNTAX.has(id) ||
        WASM.has(id) ||
        FILE.test(id)
      ) {
        continue;
      }
      if (!ids.has(id) && !groups.has(id)) {
        stale.push(`${file}:${index + 1}: \`${id}\``);
      }
    }
  });
}

if (stale.length > 0) {
  process.stderr.write(
    `${stale.length} citation(s) of spec rules the spec does not have:\n${stale.join("\n")}\n`,
  );
  process.exitCode = 1;
}
