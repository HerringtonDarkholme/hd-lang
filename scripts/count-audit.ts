// Merge JSONL from HD_COUNT_AUDIT_REPORT or the driver's count_audit example.
// node scripts/count-audit.ts target/count-audit.jsonl ...
const { readFileSync } = process.getBuiltinModule("node:fs");
import { rankCountPairs } from "../host/checked-report.ts";
import type { CountPairRow as Row } from "../host/checked-report.ts";
const totals = new Map<string, Row>();
for (const path of process.argv.slice(2)) {
  for (const text of readFileSync(path, "utf8").split("\n").filter(Boolean)) {
    const { sites } = JSON.parse(text) as { sites: Row[] };
    for (const row of sites) {
      const key = JSON.stringify([row.function, row.file, row.line, row.reason]);
      const previous = totals.get(key);
      totals.set(key, { ...row, count: String(BigInt(row.count) + BigInt(previous?.count ?? 0)) });
    }
  }
}
const rows = rankCountPairs([...totals.values()]);
console.log(
  "| Emitting function | Source | Reason | Redundant pairs |\n| --- | --- | --- | ---: |",
);
for (const row of rows) {
  const escape = (text: string) =>
    text
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll("|", "\\|")
      .replaceAll("\n", " ");
  console.log(
    `| ${escape(row.function)} | ${escape(row.file)}:${row.line} | ${escape(row.reason)} | ${row.count} |`,
  );
}
