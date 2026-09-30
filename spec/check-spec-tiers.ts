// Cross-tier name check (spec/conformance/README.md, Tiers).
//
// A case whose `specification` column cites a `std/` path is stdlib tier;
// every other case is language tier. A language-tier fixture fails when one
// of its `use std.` lines imports an item listed in
// conformance/stdlib-items.tsv, unless conformance/tier-crossings.tsv lists
// that fixture and item. Each crossing row must still be true: the case is
// language tier, and the fixture still imports or calls the item it names.
//
// Usage: node --experimental-strip-types spec/check-spec-tiers.ts SPEC_DIR
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const specDir = resolve(process.argv[2] ?? ".");
const conformance = resolve(specDir, "conformance");
const problems: string[] = [];

function readTsv(name: string, header: string): string[][] {
  const lines = readFileSync(resolve(conformance, name), "utf8").split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.shift() !== header) throw new Error(`conformance/${name}: header must be ${header}`);
  const width = header.split("\t").length;
  return lines.map((line, index) => {
    const fields = line.split("\t");
    if (fields.length !== width || fields.some((field) => field.trim() === ""))
      throw new Error(`conformance/${name}:${index + 2}: expected ${width} non-empty fields`);
    return fields;
  });
}

/**
 * Every std path a fixture imports: `use std.a.b` and `use std.a.{b, c as d}`.
 * A wildcard `use std.a.*` is a syntax error (Use Forms), so it imports nothing.
 */
function importedStdPaths(source: string): string[] {
  const paths: string[] = [];
  const pattern = /^[ \t]*(?:pub[ \t]+)?use[ \t]+(std(?:\.\w+)*)(?:\.(\{[^}]*\}|\*))?/gm;
  for (const match of source.matchAll(pattern)) {
    const base = match[1]!;
    const tail = match[2];
    if (tail === "*") continue;
    if (!tail) {
      paths.push(base);
      continue;
    }
    for (const element of tail.slice(1, -1).split(",")) {
      const name = element
        .trim()
        .split(/\s+as\s+/)[0]!
        .trim();
      if (name !== "") paths.push(`${base}.${name}`);
    }
  }
  return paths;
}

/** Two std paths overlap when one is the other or a module that contains it. */
function overlaps(left: string, right: string): boolean {
  return left === right || left.startsWith(`${right}.`) || right.startsWith(`${left}.`);
}

const cases = new Map(
  readTsv("cases.tsv", "path\tphase\texpectation\tspecification").map(
    ([path, , , specification]) => [path!, specification!],
  ),
);
const stdlibItems = readTsv("stdlib-items.tsv", "item\tspecification");
const crossings = readTsv("tier-crossings.tsv", "path\titems\treason");

for (const [item, specification] of stdlibItems) {
  if (!/^std(\.\w+)+$/.test(item!))
    problems.push(`stdlib-items.tsv: ${item} is not a std path such as std.time.s`);
  if (
    !specification!.startsWith("std/") ||
    !existsSync(resolve(specDir, specification!.split("#")[0]!))
  )
    problems.push(
      `stdlib-items.tsv: ${item} must cite an existing std/ chapter, not ${specification}`,
    );
}

const sources = new Map<string, string>();
function source(path: string): string {
  let text = sources.get(path);
  if (text === undefined) {
    text = readFileSync(resolve(conformance, path), "utf8");
    sources.set(path, text);
  }
  return text;
}

const allowed = new Map<string, string[]>();
const seen = new Set<string>();
for (const [path, itemList] of crossings) {
  const specification = cases.get(path!);
  if (seen.has(path!)) problems.push(`tier-crossings.tsv: duplicate row for ${path}`);
  seen.add(path!);
  if (specification === undefined) {
    problems.push(`tier-crossings.tsv: ${path} is not a case in cases.tsv`);
    continue;
  }
  if (specification.startsWith("std/"))
    problems.push(`tier-crossings.tsv: ${path} is stdlib tier now; delete its row`);
  const items = itemList!.split(",").map((item) => item.trim());
  const imports = importedStdPaths(source(path!));
  for (const item of items) {
    // `std.module.name` is an import; `Type.method` is a method call.
    const present = item.startsWith("std.")
      ? imports.some((imported) => overlaps(imported, item))
      : /^\w+\.\w+$/.test(item) && source(path!).includes(`.${item.split(".")[1]}(`);
    if (!present)
      problems.push(`tier-crossings.tsv: ${path} no longer uses ${item}; update or delete its row`);
  }
  allowed.set(path!, items);
}

for (const [path, specification] of cases) {
  if (specification.startsWith("std/")) continue;
  for (const imported of importedStdPaths(source(path))) {
    const item = stdlibItems.find(([listed]) => overlaps(imported, listed!));
    if (!item) continue;
    if ((allowed.get(path) ?? []).some((entry) => overlaps(entry, imported))) continue;
    problems.push(
      `${path}: language-tier case (${specification}) imports ${imported}, a stdlib-tier item of ${item[1]}; ` +
        "drop the import, cite a std/ section, or record it in tier-crossings.tsv",
    );
  }
}

if (problems.length) {
  for (const problem of problems) console.error(`spec tiers: ${problem}`);
  process.exitCode = 1;
}
