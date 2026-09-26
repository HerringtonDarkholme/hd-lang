// Row-change ripple measurement over a 31-function binary call tree.
// Writes three programs (base, edited, edited-fixed), runs `hd check` and
// `hd dump-hir` on each, and reports which functions' and closures' rows change.
// Usage: node --experimental-strip-types row-ripple.ts OUT_DIR
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const COUNT = 31;
const LEAF = 30;
const isLeaf = (index: number): boolean => 2 * index + 1 >= COUNT;

function ancestors(index: number): Set<number> {
  const result = new Set<number>();
  let current = index;
  while (current > 0) {
    current = Math.floor((current - 1) / 2);
    result.add(current);
  }
  return result;
}

function program(editLeaf: boolean, fixAncestors: boolean): string {
  const carriers = fixAncestors ? ancestors(LEAF) : new Set<number>();
  const lines = [
    "trait Logger:",
    "    fn write(self, message: string) -> void",
    "",
    "trait Cache:",
    "    fn size(self) -> i32",
    "",
    "data L: pass",
    "data C: pass",
    "",
    "impl Logger for L:",
    "    fn write(self, message: string) -> void: pass",
    "",
    "impl Cache for C:",
    "    fn size(self) -> i32: 1",
    "",
  ];
  for (let index = 0; index < COUNT; index += 1) {
    const cache = (editLeaf && index === LEAF) || carriers.has(index);
    const row = cache ? "$ Logger + Cache" : "$ Logger";
    lines.push(`fn f${index}() -> i32 ${row}:`);
    if (isLeaf(index)) {
      lines.push('    $.use(Logger).write("leaf")');
      lines.push(editLeaf && index === LEAF ? "    $.use(Cache).size()" : "    1");
    } else {
      const left = 2 * index + 1;
      const right = 2 * index + 2;
      lines.push(`    cb := fn(): f${right}()`);
      lines.push(`    f${left}() + cb()`);
    }
    lines.push("");
  }
  lines.push("fn main() -> i32:");
  lines.push("    $.with(Logger=L {}, Cache=C {}):");
  lines.push("        f0()");
  return `${lines.join("\n")}\n`;
}

interface HirRows {
  readonly functions: ReadonlyMap<string, string>;
  readonly closures: ReadonlyMap<string, string>;
}

function run(command: string, file: string): { status: number; stdout: string; stderr: string } {
  const result = spawnSync("node", ["--experimental-strip-types", "bin/hd.js", command, file], {
    encoding: "utf8",
    maxBuffer: 1 << 28,
  });
  return { status: result.status ?? -1, stdout: result.stdout, stderr: result.stderr };
}

function rows(file: string): HirRows | undefined {
  const dumped = run("dump-hir", file);
  if (dumped.status !== 0) return undefined;
  const hir = JSON.parse(dumped.stdout) as {
    functions: { name: string; requirements: string[] }[];
    closures: { name: string; requirements: string[] }[];
  };
  const pick = (list: { name: string; requirements: string[] }[]): Map<string, string> =>
    new Map(list.map((entry) => [entry.name, entry.requirements.join("+")]));
  return { functions: pick(hir.functions), closures: pick(hir.closures) };
}

const outDir = process.argv[2];
if (outDir === undefined) throw new Error("usage: row-ripple.ts OUT_DIR");
mkdirSync(outDir, { recursive: true });
const variants = [
  ["base", false, false],
  ["edited", true, false],
  ["edited-fixed", true, true],
] as const;
const results = new Map<string, HirRows | undefined>();
for (const [name, edit, fix] of variants) {
  const file = join(outDir, `row-ripple-${name}.hd`);
  writeFileSync(file, program(edit, fix));
  const checked = run("check", file);
  const errors = checked.stderr.split("\n").filter((line) => line.includes(": ") && !line.includes("warning"));
  console.log(`## ${name}: check exit ${checked.status}, ${errors.length} error diagnostics`);
  for (const line of errors) console.log(`    ${line}`);
  results.set(name, rows(file));
}
const base = results.get("base")!;
const fixed = results.get("edited-fixed")!;
const changed = (left: ReadonlyMap<string, string>, right: ReadonlyMap<string, string>): string[] =>
  [...left.keys()].filter((key) => left.get(key) !== right.get(key));
const changedFunctions = changed(base.functions, fixed.functions);
const changedClosures = changed(base.closures, fixed.closures);
console.log(`\nfunctions (named, incl. synthetic): ${base.functions.size}; closures: ${base.closures.size}`);
console.log(`named functions whose rows changed base -> edited-fixed: ${changedFunctions.length}: ${changedFunctions.join(", ")}`);
console.log(`closures whose inferred rows changed: ${changedClosures.length}: ${changedClosures.map((key) => `${key}(${base.closures.get(key)} -> ${fixed.closures.get(key)})`).join(", ")}`);
console.log(`source signatures edited by hand to restore the build: ${ancestors(LEAF).size} (ancestors of f${LEAF})`);
