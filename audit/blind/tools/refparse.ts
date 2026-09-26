// Runs the specification reference parser over blind fixtures.
// Usage: node --experimental-strip-types audit/blind/tools/refparse.ts [files...]
// With no arguments, parses every fixture listed in audit/blind/MANIFEST.tsv and
// checks that phase=parse reject rows are rejected and all other rows parse.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseSource } from "../../../spec/reference-parser/parser.ts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

interface Row {
  readonly path: string;
  readonly phase: string;
  readonly expectation: string;
}

function manifestRows(): Row[] {
  const text = readFileSync(resolve(root, "MANIFEST.tsv"), "utf8");
  return text
    .split(/\r?\n/)
    .slice(1)
    .filter(Boolean)
    .map((line) => {
      const [path = "", phase = "", expectation = ""] = line.split("\t");
      return { expectation, path, phase };
    });
}

const args = process.argv.slice(2);
const rows: Row[] =
  args.length > 0
    ? args.map((path) => ({ expectation: "accept", path, phase: "?" }))
    : manifestRows();

let failures = 0;
for (const row of rows) {
  const file = args.length > 0 ? resolve(row.path) : resolve(root, row.path);
  const diagnostics = parseSource(readFileSync(file, "utf8"));
  const shouldReject = row.phase === "parse" && row.expectation.startsWith("reject");
  const rejected = diagnostics.length > 0;
  const ok = shouldReject === rejected;
  if (!ok) failures += 1;
  const detail = diagnostics.map((d) => `${d.code}@${d.line}`).join(", ");
  console.log(`${ok ? "ok  " : "FAIL"} ${row.path}${detail ? `  [${detail}]` : ""}`);
}
console.log(`${rows.length - failures}/${rows.length} match expectation`);
process.exitCode = failures === 0 ? 0 : 1;
