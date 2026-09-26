// Audit 1.2: compare each changed fixture's marker line with the line the
// repository compiler reports. Reads markers-raw.tsv (from markers.ts).
// Usage: node --experimental-strip-types audit/evidence/01-fixtures/impl-lines.ts > audit/evidence/01-fixtures/impl-lines.tsv
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

const root = new URL("../../../", import.meta.url).pathname;
const raw = readFileSync(`${root}audit/evidence/01-fixtures/markers-raw.tsv`, "utf8")
  .trim()
  .split("\n")
  .filter((line) => !line.startsWith("#"))
  .slice(1)
  .map((line) => line.split("\t"));
const selected = new Set(
  readFileSync(`${root}test/portable/cases.tsv`, "utf8")
    .trim()
    .split("\n")
    .slice(1)
    .map((line) => line.split("\t")[0]),
);

function run(args: string[]): { code: number; out: string } {
  const result = spawnSync("node", ["--experimental-strip-types", "bin/hd.js", ...args], {
    cwd: root,
    encoding: "utf8",
    timeout: 60_000,
  });
  return { code: result.status ?? -1, out: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

const rows = [
  `# commit bd985d7; command: node --experimental-strip-types audit/evidence/01-fixtures/impl-lines.ts; date ${new Date().toISOString().slice(0, 10)}`,
  ["path", "selected", "command", "exit", "marker_code", "marker_line", "impl_lines_for_code", "verdict", "first_output_line"].join("\t"),
];
for (const cols of raw) {
  const [path, , , phase, expectation] = cols;
  const code = cols[8];
  const markerLine = cols[9];
  const file = `spec/conformance/${path}`;
  const source = readFileSync(`${root}${file}`, "utf8");
  const profile = /^# fixture-runtime-profile: ([a-z0-9-]+)/m.exec(source)?.[1];
  const scenario = /^# fixture-runtime-scenario: ([a-z0-9-]+)/m.exec(source)?.[1];
  const pending = /^# fixture-runtime-pending-function: ([A-Za-z0-9_]+)/m.exec(source)?.[1];
  const options = [
    ...(profile ? ["--profile", profile] : []),
    ...(phase === "runtime" && scenario ? ["--scenario", scenario] : []),
    ...(phase === "runtime" && pending ? ["--pending-function", pending] : []),
  ];
  const command = phase === "parse" ? "parse" : phase === "runtime" ? "test" : "check";
  const result = run([command, ...options, file]);
  const lines: string[] = [];
  if (code !== "-") {
    const escaped = code.replaceAll("-", "\\-");
    for (const match of result.out.matchAll(new RegExp(`:(\\d+):\\d+: (?:warning: )?${escaped}:`, "g")))
      lines.push(match[1]);
  }
  let verdict: string;
  if (expectation === "accept") verdict = result.code === 0 ? "impl-accepts" : "impl-rejects";
  else if (code === "-") verdict = "no-marker";
  else if (lines.includes(markerLine)) verdict = "impl-line-matches";
  else if (lines.length > 0) verdict = "impl-line-differs";
  else if (result.out.includes(`${code}:`)) verdict = "impl-code-unlocated";
  else verdict = "impl-code-absent";
  rows.push(
    [
      path,
      String(selected.has(path)),
      `${command} ${options.join(" ")}`.trim(),
      String(result.code),
      code,
      markerLine,
      lines.join(",") || "-",
      verdict,
      (result.out.trim().split("\n")[0] ?? "").replaceAll("\t", " ").slice(0, 160),
    ].join("\t"),
  );
}
process.stdout.write(`${rows.join("\n")}\n`);
