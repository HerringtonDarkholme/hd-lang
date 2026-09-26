// Runs spec/reference-parser parseSource over the given files and prints diagnostics.
// Usage: node --experimental-strip-types audit/probes/blind-triage/refparser/refparse-probe.ts FILE...
import { readFileSync } from "node:fs";

import { parseSource } from "../../../../spec/reference-parser/parser.ts";

for (const file of process.argv.slice(2)) {
  const diagnostics = parseSource(readFileSync(file, "utf8"));
  const detail = diagnostics.map((d) => `${d.code}@${d.line}`).join(", ");
  console.log(
    `${file}\t${diagnostics.length === 0 ? "ACCEPT" : "REJECT"}\t${detail}`,
  );
}
