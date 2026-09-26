// Generates programs with 1..MAX nested unannotated closures and times in-process analyze().
// Usage: node --experimental-strip-types nested-closures.ts OUT_DIR MAX_DEPTH
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const { analyze } = (await import(resolve("src/compiler.ts"))) as {
  analyze: (source: string) => { hir?: unknown; diagnostics: readonly unknown[] };
};

function program(depth: number): string {
  const lines = ["fn main() -> i32:"];
  const open = (level: number): void => {
    const pad = "    ".repeat(level + 1);
    if (level === depth) {
      lines.push(`${pad}1`);
      return;
    }
    lines.push(`${pad}f${level} := fn():`);
    open(level + 1);
    lines.push(`${pad}f${level}()`);
  };
  open(0);
  return `${lines.join("\n")}\n`;
}

const [outDir, maxText] = process.argv.slice(2);
if (outDir === undefined || maxText === undefined) throw new Error("usage: OUT_DIR MAX_DEPTH");
mkdirSync(outDir, { recursive: true });
console.log("| depth | check ms | ratio to previous |");
console.log("|---:|---:|---:|");
let previous = 0;
for (let depth = 1; depth <= Number(maxText); depth += 1) {
  const file = join(outDir, `nested-closures-${depth}.hd`);
  writeFileSync(file, program(depth));
  const source = program(depth);
  analyze(source);
  const start = performance.now();
  const result = analyze(source);
  const elapsed = performance.now() - start;
  if (!result.hir) throw new Error(`depth ${depth} failed: ${JSON.stringify(result.diagnostics)}`);
  console.log(`| ${depth} | ${elapsed.toFixed(0)} | ${previous > 0 ? (elapsed / previous).toFixed(2) : "-"} |`);
  previous = elapsed;
}
