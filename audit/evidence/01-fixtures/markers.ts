// Audit 1.2: fixture marker review data extraction.
// Usage: node --experimental-strip-types audit/evidence/01-fixtures/markers.ts > audit/evidence/01-fixtures/markers-raw.tsv
// Read-only: uses `git show`, `git diff`, and `git log` only.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const BASE = "4cc1312";
const HEAD = "bd985d7";
const root = new URL("../../../", import.meta.url).pathname;

function git(args: string[]): string {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 26 });
}

function gitOrEmpty(args: string[]): string {
  try {
    return git(args);
  } catch {
    return "";
  }
}

const headerPattern = /^#\s*expect-(error|warning|panic):\s*([a-z0-9-]+)\s*$/;
const inlinePattern = /#\s*(diagnostic|warning|panic):\s*([a-z0-9-]+)\s*$/;

function stripForBody(text: string): string[] {
  return text
    .split("\n")
    .filter((line) => !headerPattern.test(line.trim()))
    .map((line) => line.replace(inlinePattern, "").replace(/\s+$/, ""))
    .join("\n")
    .replace(/^\n+/, "")
    .replace(/\n+$/, "")
    .split("\n");
}

function slug(heading: string): string {
  return heading
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9 _-]/g, "")
    .replace(/ /g, "-");
}

function sectionText(ref: string): { section: string; chapter: string } {
  const [file, anchor] = ref.split("#");
  const text = readFileSync(`${root}spec/${file}`, "utf8");
  const lines = text.split("\n");
  let start = -1;
  let level = 0;
  for (let index = 0; index < lines.length; index++) {
    const match = /^(#+)\s+(.*)$/.exec(lines[index]);
    if (match && slug(match[2]) === anchor) {
      start = index;
      level = match[1].length;
      break;
    }
  }
  if (start < 0) return { section: "", chapter: text };
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index++) {
    const match = /^(#+)\s/.exec(lines[index]);
    if (match && match[1].length <= level) {
      end = index;
      break;
    }
  }
  return { section: lines.slice(start, end).join("\n"), chapter: text };
}

const cases = new Map<string, string[]>();
for (const line of readFileSync(`${root}spec/conformance/cases.tsv`, "utf8").split("\n").slice(1)) {
  if (!line.trim()) continue;
  const cols = line.split("\t");
  cases.set(cols[0], cols);
}

const changed = git([
  "diff",
  "--name-status",
  "--no-renames",
  BASE,
  HEAD,
  "--",
  "spec/conformance",
])
  .trim()
  .split("\n")
  .map((line) => line.split("\t"))
  .filter(([, path]) => path.endsWith(".hd"));

const header = [
  "path",
  "status",
  "commits",
  "phase",
  "expectation",
  "section",
  "old_header_code",
  "marker_kind",
  "marker_code",
  "marker_line",
  "marker_count",
  "code_match_cases",
  "code_match_old",
  "body_changed",
  "body_diff_lines",
  "code_in_section",
  "code_in_chapter",
  "marker_line_text",
];
const rows: string[] = [
  `# commit ${HEAD} base ${BASE}; command: node --experimental-strip-types audit/evidence/01-fixtures/markers.ts; date ${new Date().toISOString().slice(0, 10)}`,
  header.join("\t"),
];

for (const [status, path] of changed) {
  const rel = path.replace(/^spec\/conformance\//, "");
  const commits = git(["log", "--format=%h", `${BASE}..${HEAD}`, "--", path])
    .trim()
    .split("\n")
    .join(",");
  const now = git(["show", `${HEAD}:${path}`]);
  const before = status === "A" ? "" : gitOrEmpty(["show", `${BASE}:${path}`]);
  const oldCodes = before
    .split("\n")
    .map((line) => headerPattern.exec(line.trim()))
    .filter((match) => match !== null)
    .map((match) => `${match[1]}:${match[2]}`);
  const markers: { kind: string; code: string; line: number; text: string }[] = [];
  now.split("\n").forEach((line, index) => {
    const match = inlinePattern.exec(line);
    if (match) markers.push({ kind: match[1], code: match[2], line: index + 1, text: line.trim() });
  });
  const row = cases.get(rel) ?? [rel, "?", "?", "?"];
  const expectation = row[2];
  const expectedCode = expectation.includes(":") ? expectation.split(":")[1] : "";
  const markerCodes = markers.map((marker) => marker.code);
  const oldBody = stripForBody(before);
  const newBody = stripForBody(now);
  let bodyDiff = 0;
  if (status !== "A") {
    const oldSet = new Map<string, number>();
    for (const line of oldBody) oldSet.set(line, (oldSet.get(line) ?? 0) + 1);
    for (const line of newBody) {
      const count = oldSet.get(line) ?? 0;
      if (count > 0) oldSet.set(line, count - 1);
      else bodyDiff++;
    }
    for (const count of oldSet.values()) bodyDiff += count;
  }
  const bodyChanged = status === "A" ? "new" : bodyDiff > 0 ? "yes" : "no";
  const { section, chapter } = sectionText(row[3]);
  const probe = expectedCode || markerCodes[0] || "";
  const oldCode = oldCodes.map((code) => code.split(":")[1]).join(",");
  rows.push(
    [
      rel,
      status,
      commits,
      row[1],
      expectation,
      row[3],
      oldCodes.join(",") || "-",
      markers.map((marker) => marker.kind).join(",") || "-",
      markerCodes.join(",") || "-",
      markers.map((marker) => String(marker.line)).join(",") || "-",
      String(markers.length),
      expectedCode ? String(markerCodes.includes(expectedCode) && markers.length === 1) : markers.length === 0 ? "true(no-code)" : "false(marker-on-accept)",
      status === "A" ? "n/a" : oldCode === markerCodes.join(",") ? "true" : `false(${oldCode || "-"}->${markerCodes.join(",") || "-"})`,
      bodyChanged,
      String(bodyDiff),
      probe ? String(section.includes(probe)) : "n/a",
      probe ? String(chapter.includes(probe)) : "n/a",
      markers.map((marker) => marker.text.replace(/\t/g, " ")).join(" | ") || "-",
    ].join("\t"),
  );
}
process.stdout.write(`${rows.join("\n")}\n`);
