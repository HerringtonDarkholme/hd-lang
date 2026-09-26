// Measures how much of the emitted WAT and HIR changes after small source edits.
// Counts `(func` blocks whose text changed, compared by position-independent
// multiset of function texts (so pure reordering is not counted as a change).
// Usage: node --experimental-strip-types wat-stability.ts BASE.hd
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const { compile } = (await import(resolve("src/compiler.ts"))) as {
  compile: (source: string) => { wat: string; hir: { closures: { name: string }[] } };
};

function functions(wat: string): string[] {
  const blocks: string[] = [];
  let current: string[] | undefined;
  for (const line of wat.split("\n")) {
    if (/^\s*\(func /.test(line)) {
      if (current) blocks.push(current.join("\n"));
      current = [line];
    } else if (current) current.push(line);
  }
  if (current) blocks.push(current.join("\n"));
  return blocks;
}

function changedCount(before: string[], after: string[]): number {
  const pool = new Map<string, number>();
  for (const block of before) pool.set(block, (pool.get(block) ?? 0) + 1);
  let changed = 0;
  for (const block of after) {
    const available = pool.get(block) ?? 0;
    if (available > 0) pool.set(block, available - 1);
    else changed += 1;
  }
  return changed;
}

const file = process.argv[2];
if (file === undefined) throw new Error("usage: wat-stability.ts BASE.hd");
const base = readFileSync(file, "utf8");
const edits: readonly (readonly [string, string])[] = [
  ["no edit (determinism)", base],
  ["change one literal in the middle unit", base.replace("p := P50 { x: a + 3 }", "p := P50 { x: a + 7 }")],
  ["add one blank line + comment inside a function", base.replace("    p := P50 {", "    # note\n\n    p := P50 {")],
  ["insert a new function before all others", `fn helper() -> i32: 1\n\n${base}`],
  ["insert one closure in the first function", base.replace("    p := P0 { x: a + 3 }", "    k := fn(): 1\n    p := P0 { x: a + k() }")],
];
const baseBlocks = functions(compile(base).wat);
console.log(`base: ${baseBlocks.length} WAT functions`);
console.log("| edit | WAT functions after | function bodies with changed text |");
console.log("|---|---:|---:|");
for (const [label, source] of edits) {
  if (source === base && label !== "no edit (determinism)") throw new Error(`edit '${label}' did not apply`);
  const blocks = functions(compile(source).wat);
  console.log(`| ${label} | ${blocks.length} | ${changedCount(baseBlocks, blocks)} |`);
}
