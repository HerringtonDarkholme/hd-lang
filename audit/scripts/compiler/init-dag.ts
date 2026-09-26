// Times in-process analyze() on a program with a top-level binding that calls a
// chain of functions where each calls the next twice (a DAG with 2^n paths).
// Usage: node --experimental-strip-types init-dag.ts OUT_DIR N1,N2,...
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const { analyze } = (await import(resolve("src/compiler.ts"))) as {
  analyze: (source: string) => { hir?: unknown; diagnostics: readonly unknown[] };
};

function program(depth: number, topLevel: boolean): string {
  const lines: string[] = [];
  for (let index = 0; index < depth; index += 1)
    lines.push(`fn f${index}() -> i32:`, `    f${index + 1}() + f${index + 1}()`, "");
  lines.push(`fn f${depth}() -> i32:`, "    1", "");
  if (topLevel) lines.push("total := f0()", "");
  lines.push("fn main() -> i32:", topLevel ? "    total" : "    f0()", "");
  return lines.join("\n");
}

const [outDir, depthsText] = process.argv.slice(2);
if (outDir === undefined || depthsText === undefined) throw new Error("usage: OUT_DIR N1,N2");
mkdirSync(outDir, { recursive: true });
console.log("| chain depth | with top-level binding ms | without top-level binding ms |");
console.log("|---:|---:|---:|");
for (const depth of depthsText.split(",").map(Number)) {
  const times = [true, false].map((topLevel) => {
    const source = program(depth, topLevel);
    writeFileSync(join(outDir, `init-dag-${depth}-${topLevel ? "top" : "plain"}.hd`), source);
    const start = performance.now();
    const result = analyze(source);
    if (!result.hir) throw new Error(JSON.stringify(result.diagnostics));
    return performance.now() - start;
  });
  console.log(`| ${depth} | ${times[0]!.toFixed(0)} | ${times[1]!.toFixed(0)} |`);
}
