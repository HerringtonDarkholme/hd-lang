// Code-health duplication metrics for src/.
// 1. Shingle duplication: windows of W normalized non-trivial lines that occur
//    more than once, reported per file and per file pair.
// 2. HIR walker census: how many HIR expression kinds each file switches on
//    (`case "<kind>"`), i.e. how many parallel HIR traversals exist.
// 3. Variant census: counts of kind-name families (static/dynamic/trait/stored
//    suspension variants) per file.
// Usage: node --experimental-strip-types duplication.ts [W=6]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const WINDOW = Number(process.argv[2] ?? "6");
const root = resolve("src");

function walk(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) return walk(path);
    return path.endsWith(".ts") ? [path] : [];
  });
}

const files = walk(root);
const normalized = new Map<string, { text: string; line: number }[]>();
for (const file of files) {
  const lines = readFileSync(file, "utf8").split("\n");
  const kept = lines
    .map((text, index) => ({ text: text.trim().replaceAll(/\s+/g, " "), line: index + 1 }))
    .filter((entry) => entry.text.length >= 12 && !entry.text.startsWith("//") && !entry.text.startsWith("import"));
  normalized.set(relative(resolve("."), file), kept);
}

const occurrences = new Map<string, { file: string; line: number }[]>();
for (const [file, lines] of normalized) {
  for (let index = 0; index + WINDOW <= lines.length; index += 1) {
    const key = lines
      .slice(index, index + WINDOW)
      .map((entry) => entry.text)
      .join("\n");
    if (!occurrences.has(key)) occurrences.set(key, []);
    occurrences.get(key)!.push({ file, line: lines[index]!.line });
  }
}

const duplicatedLines = new Map<string, Set<number>>();
const pairs = new Map<string, number>();
for (const [key, places] of occurrences) {
  if (places.length < 2) continue;
  const span = key.split("\n").length;
  for (const place of places) {
    if (!duplicatedLines.has(place.file)) duplicatedLines.set(place.file, new Set());
    for (let offset = 0; offset < span; offset += 1) duplicatedLines.get(place.file)!.add(place.line + offset);
  }
  const distinct = [...new Set(places.map((place) => place.file))].toSorted();
  const pairKey = distinct.length === 1 ? `${distinct[0]} (self)` : distinct.join(" <-> ");
  pairs.set(pairKey, (pairs.get(pairKey) ?? 0) + 1);
}

console.log(`## Shingle duplication (window ${WINDOW} normalized lines of >= 12 chars)`);
console.log("| file | kept lines | lines inside a duplicated window | % |");
console.log("|---|---:|---:|---:|");
let totalKept = 0;
let totalDuplicated = 0;
for (const [file, lines] of normalized) {
  const duplicated = duplicatedLines.get(file)?.size ?? 0;
  totalKept += lines.length;
  totalDuplicated += duplicated;
  if (duplicated > 0)
    console.log(`| ${file} | ${lines.length} | ${duplicated} | ${((duplicated / lines.length) * 100).toFixed(1)}% |`);
}
console.log(`| **total** | ${totalKept} | ${totalDuplicated} | ${((totalDuplicated / totalKept) * 100).toFixed(1)}% |`);
console.log("\n| file pair | duplicated windows |");
console.log("|---|---:|");
for (const [pair, count] of [...pairs].toSorted((left, right) => right[1] - left[1]).slice(0, 15))
  console.log(`| ${pair} | ${count} |`);

const hirText = readFileSync(join(root, "hir.ts"), "utf8");
const expressionStart = hirText.indexOf("export type HirExpression =");
const hirKinds = new Set(
  [...hirText.slice(expressionStart).matchAll(/kind: "([a-z-]+)"/g)].map((match) => match[1]!),
);
console.log(`\n## HIR walker census (${hirKinds.size} HIR expression kinds)`);
console.log("| file | distinct HIR expression kinds in `case` labels |");
console.log("|---|---:|");
for (const file of files) {
  const text = readFileSync(file, "utf8");
  const cases = new Set([...text.matchAll(/case "([a-z-]+)"/g)].map((match) => match[1]!).filter((kind) => hirKinds.has(kind)));
  if (cases.size >= 10) console.log(`| ${relative(resolve("."), file)} | ${cases.size} |`);
}

const families: Readonly<Record<string, RegExp>> = {
  "suspend-*/suspension-*/trait-suspend-*": /"(?:trait-)?suspen(?:d|sion)-[a-z]+"/g,
  "trait-call/closure-call/call": /"(?:trait-call|closure-call|call)"/g,
};
console.log("\n## Variant-kind references per file");
console.log(`| file | ${Object.keys(families).join(" | ")} |`);
console.log(`|---|${Object.keys(families).map(() => "---:").join("|")}|`);
for (const file of files) {
  const text = readFileSync(file, "utf8");
  const counts = Object.values(families).map((pattern) => text.match(pattern)?.length ?? 0);
  if (counts.some((count) => count >= 5)) console.log(`| ${relative(resolve("."), file)} | ${counts.join(" | ")} |`);
}
