// Import graph over src/: per-file lines, fan-in, fan-out, and folder-boundary
// violations (imports of parser/checker/emitter internals from outside the folder).
// Usage: node --experimental-strip-types import-graph.ts [root=src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";

const root = resolve(process.argv[2] ?? "src");
const repo = resolve(root, "..");
const FOLDERS = ["parser", "checker", "emitter"];

function walk(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) return walk(path);
    return path.endsWith(".ts") ? [path] : [];
  });
}

const scanned = [...walk(root), ...walk(join(repo, "test")), ...walk(join(repo, "spec")), join(repo, "bin")]
  .filter((path) => path.endsWith(".ts"));
const importPattern = /(?:import|export)\s[^;]*?from\s+"(\.[^"]+)"/gs;
const fanIn = new Map<string, Set<string>>();
const fanOut = new Map<string, Set<string>>();
const violations: string[] = [];

for (const file of scanned) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(importPattern)) {
    const target = resolve(dirname(file), match[1]!);
    const from = relative(repo, file);
    const to = relative(repo, target);
    if (!fanIn.has(to)) fanIn.set(to, new Set());
    fanIn.get(to)!.add(from);
    if (!fanOut.has(from)) fanOut.set(from, new Set());
    fanOut.get(from)!.add(to);
    for (const folder of FOLDERS) {
      const folderPath = join(root, folder);
      const insideTarget = target.startsWith(folderPath + "/");
      const insideSource = file.startsWith(folderPath + "/");
      if (insideTarget && !insideSource && !target.endsWith("/index.ts"))
        violations.push(`${from} -> ${to}`);
    }
  }
}

const rows = walk(root)
  .map((file) => {
    const name = relative(repo, file);
    const lines = readFileSync(file, "utf8").split("\n").length - 1;
    return { name, lines, fanIn: fanIn.get(name)?.size ?? 0, fanOut: fanOut.get(name)?.size ?? 0 };
  })
  .toSorted((left, right) => right.fanIn - left.fanIn || right.lines - left.lines);

console.log("| file | lines | fan-in | fan-out | % of 1500 cap |");
console.log("|---|---:|---:|---:|---:|");
for (const row of rows)
  console.log(`| ${row.name} | ${row.lines} | ${row.fanIn} | ${row.fanOut} | ${Math.round((row.lines / 1500) * 100)}% |`);
console.log(`\nboundary violations (${violations.length}):`);
for (const violation of violations) console.log(`  ${violation}`);
for (const target of ["src/checker/shared.ts", "src/checker/context.ts", "src/checker/expression-calls.ts"])
  console.log(`\nimporters of ${target}: ${[...(fanIn.get(target) ?? [])].toSorted().join(", ")}`);
