// Fails when a per-run path lists a directory or spawns a process per item
// without stating its cost (.agents/skills/shared-rules.md "Cost Model").
//
// The rules are ast-grep rules in .ast-grep/rules/ (.ast-grep/sgconfig.yml):
// - cost-rust-read-dir: `read_dir(...)` in non-test code under crates/*/src.
// - cost-rust-spawn-in-loop: `Command::new(...)` in a loop body or an
//   iterator closure, in the same code.
// - cost-ts-readdir: `readdir`/`readdirSync`/`opendir` under scripts/,
//   spec/, website/, packages/ (non-test).
// - cost-ts-spawn-in-loop: `spawn`/`spawnSync`/`execFile`/`execFileSync`/
//   `execSync` in a loop body, an array callback (`.map`, `.forEach`, ...),
//   or a `new Promise` executor (the per-item process helper), same files.
//
// A hit passes when a `cost:` comment is on its first line or on one of the
// 3 lines above, stating the cost per run. A hit whose cost is proportional
// to accumulated state is a defect: it is listed in scripts/cost-allowlist.tsv
// with the issue that removes it (`untracked` until one is filed). A row
// that no longer matches fails too, so the list only shrinks.
//
// Cost per run: one `ast-grep scan` process over the repository (parallel,
// about 0.5 s), then one read of each file that has a hit (tens of files).
//
// Usage: node scripts/check-cost.ts

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const ALLOWLIST = "scripts/cost-allowlist.tsv";
const ABOVE = 3;
const COST_COMMENT = /(\/\/|\/\*|^\s*\*).*\bcost:/;

type Match = {
  ruleId: string;
  file: string;
  message: string;
  lines: string;
  range: { start: { line: number } };
};

type Row = { rule: string; path: string; issue: string; code: string; line: number };

const scan = spawnSync(
  "ast-grep",
  ["scan", "--config", ".ast-grep/sgconfig.yml", "--json=compact"],
  {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  },
);
if (scan.error) {
  process.stderr.write(`check:cost needs ast-grep on PATH: ${scan.error.message}\n`);
  process.exit(2);
}
// ast-grep exits 1 when an error-severity rule matches; anything else is a
// broken run (a bad rule or config).
if (scan.status !== 0 && scan.status !== 1) {
  process.stderr.write(`ast-grep scan failed (status ${scan.status}):\n${scan.stderr}\n`);
  process.exit(2);
}
const matches = JSON.parse(scan.stdout) as Match[];

const rows: Row[] = readFileSync(join(root, ALLOWLIST), "utf8")
  .split("\n")
  .map((text, index) => ({ text, line: index + 1 }))
  .slice(1)
  .filter(({ text }) => text.trim() !== "")
  .map(({ text, line }) => {
    const [rule = "", path = "", issue = "", code = ""] = text.split("\t");
    return { rule, path, issue, code, line };
  });

const failures: string[] = [];
for (const row of rows) {
  if (!/^(#\d+|untracked)$/.test(row.issue)) {
    failures.push(
      `${ALLOWLIST}:${row.line}: the issue column must be an issue number, as #1502, or untracked`,
    );
  }
}

const key = (rule: string, path: string, code: string): string => `${rule}\t${path}\t${code}`;
const allowed = new Map<string, Row[]>();
for (const row of rows) {
  const k = key(row.rule, row.path, row.code);
  allowed.set(k, [...(allowed.get(k) ?? []), row]);
}

const texts = new Map<string, string[]>();
const linesOf = (file: string): string[] => {
  let lines = texts.get(file);
  if (!lines) {
    lines = readFileSync(join(root, file), "utf8").split("\n");
    texts.set(file, lines);
  }
  return lines;
};

for (const match of matches) {
  // ban-unsupported has no escape: scripts/check-unsupported.ts counts it.
  if (match.ruleId === "ban-unsupported") {
    continue;
  }
  const start = match.range.start.line;
  const lines = linesOf(match.file);
  const stated = lines
    .slice(Math.max(0, start - ABOVE), start + 1)
    .some((text) => COST_COMMENT.test(text));
  if (stated) {
    continue;
  }
  const code = (lines[start] ?? "").trim();
  const listed = allowed.get(key(match.ruleId, match.file, code));
  if (listed && listed.length > 0) {
    listed.shift();
    continue;
  }
  failures.push(`${match.file}:${start + 1}: ${match.ruleId}: ${match.message}\n    ${code}`);
}

for (const left of allowed.values()) {
  for (const row of left) {
    failures.push(
      `${ALLOWLIST}:${row.line}: ${row.rule} ${row.path} no longer matches an unstated hit; delete the row (${row.issue})`,
    );
  }
}

if (failures.length > 0) {
  process.stderr.write(
    `check:cost: ${failures.length} per-run cost${failures.length === 1 ? "" : "s"} not stated.\n` +
      `Add a \`cost:\` comment (on the line or the ${ABOVE} above) saying what the run costs as a\n` +
      `function of the change and of accumulated state, or fix the cost (Cost Model,\n` +
      `.agents/skills/shared-rules.md).\n\n${failures.join("\n")}\n`,
  );
  process.exitCode = 1;
}
