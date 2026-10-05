// Static source census. Counts import declarations, not a complete module graph:
// re-exports, dynamic imports, external dependencies, and test consumers are excluded.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, normalize, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}
const records = walk(join(root, "src"))
  .filter((file) => /\.(ts|wat)$/.test(file))
  .map((file) => {
    const source = readFileSync(file, "utf8");
    return {
      file: relative(root, file),
      lines: source.split("\n").length - Number(source.endsWith("\n")),
      imports: [...source.matchAll(/^import\s+(type\s+)?[\s\S]*?from\s+"([^"]+)";/gm)].map(
        (match) => ({
          module: match[2],
          typeOnly: !!match[1],
          line: source.slice(0, match.index).split("\n").length,
        }),
      ),
    };
  });
const fileSet = new Set(records.map((record) => record.file));
const groups = {};
const edges = [];
for (const record of records) {
  const group = record.file.includes("/runtime/")
    ? "emitter/runtime"
    : record.file.split("/").length === 2
      ? "root"
      : record.file.split("/")[1];
  groups[group] ??= { files: 0, lines: 0 };
  groups[group].files++;
  groups[group].lines += record.lines;
  for (const imported of record.imports) {
    if (!imported.module.startsWith(".")) continue;
    const target = normalize(join(dirname(record.file), imported.module));
    if (fileSet.has(target))
      edges.push({
        from: record.file,
        to: target,
        typeOnly: imported.typeOnly,
        line: imported.line,
      });
  }
}
const incoming = new Map(records.map((record) => [record.file, 0]));
for (const edge of edges) incoming.set(edge.to, incoming.get(edge.to) + 1);
const catalog = readFileSync(new URL("modules-2026-10-04.md", import.meta.url), "utf8");
const documented = [...catalog.matchAll(/\]\(\.\.\/\.\.\/(src\/[^)]+\.(?:ts|wat))\)/g)].map(
  (match) => match[1],
);
const missing = [...fileSet].filter((file) => !documented.includes(file));
const unknown = documented.filter((file) => !fileSet.has(file));
const duplicate = [
  ...new Set(documented.filter((file, index) => documented.indexOf(file) !== index)),
];
const invalidLinks = [];
const unbalancedFences = [];
let localLinks = 0;
for (const name of [
  "status-quo-2026-10-04.md",
  "modules-2026-10-04.md",
  "checker-2026-10-04.md",
  "emitter-2026-10-04.md",
  "findings-2026-10-04.md",
]) {
  const document = new URL(name, import.meta.url);
  const source = readFileSync(document, "utf8");
  if ([...source.matchAll(/^```/gm)].length % 2 !== 0) unbalancedFences.push(name);
  for (const match of source.matchAll(/\]\(([^)]+)\)/g)) {
    if (/^(?:https?:|#)/.test(match[1])) continue;
    localLinks++;
    const target = new URL(match[1].split("#")[0], document);
    if (!existsSync(target)) invalidLinks.push({ document: name, target: match[1] });
  }
}
console.log(
  JSON.stringify(
    {
      groups,
      files: records.length,
      lines: records.reduce((count, record) => count + record.lines, 0),
      sourceImportDeclarations: edges.length,
      topIncoming: [...incoming].sort((left, right) => right[1] - left[1]).slice(0, 12),
      boundaryCrossings: edges.filter((edge) =>
        ["checker", "parser", "emitter"].some(
          (group) =>
            edge.to.startsWith(`src/${group}/`) &&
            !edge.from.startsWith(`src/${group}/`) &&
            !edge.to.endsWith("/index.ts"),
        ),
      ),
      catalog: { documented: documented.length, missing, unknown, duplicate },
      documents: { localLinks, invalidLinks, unbalancedFences },
    },
    null,
    2,
  ),
);
if (
  missing.length ||
  unknown.length ||
  duplicate.length ||
  invalidLinks.length ||
  unbalancedFences.length
)
  process.exitCode = 1;
