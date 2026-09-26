// Prints reference-parser diagnostics (CODE:LINE) for each .hd path given.
// Usage: node --experimental-strip-types audit/scripts/w6/refparse.ts FILE...
import { readFileSync } from "node:fs";

import { parseSource } from "../../../spec/reference-parser/parser.ts";

for (const path of process.argv.slice(2)) {
  const diagnostics = parseSource(readFileSync(path, "utf8"));
  const rendered = diagnostics.map(({ code, line }) => `${code}:${line}`).join(" ");
  console.log(`${path}\t${rendered || "ok"}`);
}
