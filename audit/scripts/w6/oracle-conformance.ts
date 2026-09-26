// W6 rerun of the audit's oracle-strength check (audit/scripts/test-quality/oracle.ts)
// over spec/conformance only. For each marked case: (a) neutralize the marked
// line in a temporary copy and rerun the real compiler; (b) record every other
// error the original produces. Writes audit/evidence/w6/oracle-conformance.tsv
// and prints the code-persists and extra-error counts for selected cases.
// Usage: node --experimental-strip-types audit/scripts/w6/oracle-conformance.ts [--jobs N]
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";

import { type FixtureInfo, header, readConformance, root, tsv } from "../test-quality/lib.ts";

const probeRoot = resolve(tmpdir(), "hd-w6-oracle");
const out = resolve(root, "audit/evidence/w6");
const hd = ["--experimental-strip-types", resolve(root, "bin/hd.js")];
const cliProfiles = new Set(["pending-gate", "ready-counter", "ready-float", "ready-gate", "ready-text"]);

interface Run {
  readonly code: number;
  readonly output: string;
}

function invoke(args: readonly string[]): Promise<Run> {
  return new Promise((complete) => {
    const child = spawn(process.execPath, [...hd, ...args], { cwd: root, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => (output += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => (output += chunk));
    const timer = setTimeout(() => child.kill("SIGKILL"), 20_000);
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      complete({ code: code ?? (signal ? 137 : 1), output });
    });
  });
}

interface Located {
  readonly line: number;
  readonly code: string;
  readonly warning: boolean;
}

function located(output: string): Located[] {
  const result: Located[] = [];
  for (const match of output.matchAll(/^.*?:(\d+):\d+: (warning: )?([a-z0-9-]+):/gm))
    result.push({ code: match[3]!, line: Number(match[1]), warning: Boolean(match[2]) });
  return result;
}

function panics(output: string): string[] {
  return [...output.matchAll(/^([a-z0-9-]+): runtime panic/gm)].map((match) => match[1]!);
}

function phaseOf(fixture: FixtureInfo): "parse" | "type" | "runtime" {
  if (fixture.body === "spec/conformance") return fixture.phase as "parse" | "type" | "runtime";
  return fixture.markers[0]?.kind === "panic" ? "runtime" : "type";
}

async function execute(fixture: FixtureInfo, path: string): Promise<{ run: Run; stage: string }> {
  const profile = fixture.profile && cliProfiles.has(fixture.profile) ? ["--profile", fixture.profile] : [];
  const phase = phaseOf(fixture);
  if (phase === "parse") return { run: await invoke(["parse", path]), stage: "parse" };
  const checked = await invoke(["check", ...profile, path]);
  if (phase === "type" || checked.code !== 0) return { run: checked, stage: "check" };
  if (fixture.body === "test/fixtures") return { run: await invoke(["run", ...profile, path]), stage: "run" };
  const options = [
    ...(fixture.scenario ? ["--scenario", fixture.scenario] : []),
    ...(fixture.pendingFunction ? ["--pending-function", fixture.pendingFunction] : []),
    ...profile,
  ];
  return { run: await invoke(["test", ...options, path]), stage: "test" };
}

function indentation(line: string): string {
  return /^\s*/.exec(line)![0];
}

function structural(line: string): boolean {
  const code = line.replace(/#.*$/, "").trimEnd();
  return /:\s*$/.test(code) || /^\s*(pub\s+)?(fn|impl|trait|data|enum|match)\b/.test(code);
}

async function mapParallel<T, U>(values: readonly T[], jobs: number, operation: (value: T) => Promise<U>): Promise<U[]> {
  const results: U[] = [];
  let next = 0;
  async function worker(): Promise<void> {
    while (next < values.length) {
      const index = next;
      next += 1;
      results[index] = await operation(values[index]!);
    }
  }
  await Promise.all(Array.from({ length: Math.min(jobs, values.length) }, worker));
  return results;
}

async function analyze(fixture: FixtureInfo): Promise<unknown[]> {
  const marker = fixture.markers[0]!;
  const original = await execute(fixture, fixture.absolute);
  const diagnostics = located(original.run.output);
  const errors = diagnostics.filter((d) => !d.warning);
  const matched = diagnostics.filter((d) => d.code === marker.code && d.line === marker.line);
  const extras = (marker.kind === "warning" ? diagnostics : errors).filter(
    (d) => !(d.code === marker.code && d.line === marker.line),
  );
  const originalPanics = panics(original.run.output);
  // (a) neutralize the marked line: delete it; if the copy no longer parses, replace it with `pass`.
  const copies: Array<[string, string[]]> = [
    ["delete", fixture.lines.filter((_, index) => index !== marker.line - 1)],
    ["pass", fixture.lines.map((line, index) => (index === marker.line - 1 ? `${indentation(line)}pass` : line))],
  ];
  let strategy = "";
  let neutral: { run: Run; stage: string } | undefined;
  for (const [name, lines] of copies) {
    const copy = resolve(probeRoot, fixture.body === "spec/conformance" ? "conformance" : "fixtures", `${fixture.path.replace(/\.hd$/, "")}.${name}.hd`);
    mkdirSync(dirname(copy), { recursive: true });
    writeFileSync(copy, lines.join("\n"));
    const parsed = await invoke(["parse", copy]);
    strategy = name;
    neutral = await execute(fixture, copy);
    if (parsed.code === 0) break;
  }
  const after = located(neutral!.run.output);
  const afterPanics = panics(neutral!.run.output);
  const stillThere =
    marker.kind === "panic" ? afterPanics.includes(marker.code) : after.some((d) => d.code === marker.code);
  let verdict: string;
  if (stillThere) verdict = "code-persists";
  else if (neutral!.run.code === 0) verdict = "isolated-accepts";
  else verdict = `isolated-other-errors:${[...new Set(after.map((d) => d.code).concat(afterPanics))].join("+") || "unlocated"}`;
  const markerObserved =
    marker.kind === "panic" ? originalPanics.includes(marker.code) : matched.length > 0;
  return [
    fixture.body,
    fixture.path,
    `${marker.kind}:${marker.code}@${marker.line}`,
    fixture.markers.length,
    fixture.body === "spec/conformance" ? (fixture.selected ? "selected" : "unselected") : "run-portable",
    `${original.stage}:${original.run.code}`,
    markerObserved ? "yes" : "no",
    marker.kind === "panic" ? "n/a" : extras.length ? "yes" : "no",
    [...new Set(extras.map((d) => `${d.code}@${d.line}`))].join(" "),
    structural(fixture.lines[marker.line - 1] ?? "") ? "yes" : "no",
    strategy,
    `${neutral!.stage}:${neutral!.run.code}`,
    verdict,
  ];
}

async function main(): Promise<void> {
  const jobsIndex = process.argv.indexOf("--jobs");
  const jobs = jobsIndex > 0 ? Number(process.argv[jobsIndex + 1]) : 4;
  const cases = readConformance().filter((fixture) => fixture.markers.length > 0);
  const rows = await mapParallel(cases, jobs, analyze);
  const head = [
    "body", "case", "marker", "marker_count", "selection", "original_stage:exit", "marker_observed",
    "extra_errors", "extra_list", "marker_on_structural_line", "neutralize", "neutral_stage:exit", "verdict",
  ];
  writeFileSync(
    resolve(out, "oracle-conformance.tsv"),
    `${header("node --experimental-strip-types audit/scripts/w6/oracle-conformance.ts")}\n${tsv([head, ...rows])}\n`,
  );
  const selected = rows.filter((row) => row[4] === "selected");
  const persists = selected.filter((row) => row[12] === "code-persists").map((row) => row[1]);
  const extras = selected.filter((row) => row[7] === "yes").map((row) => `${row[1]} (${row[8]})`);
  console.log(`cases=${rows.length} selected=${selected.length}`);
  console.log(`selected code-persists: ${persists.length}${persists.length ? ` ${persists.join(", ")}` : ""}`);
  console.log(`selected extra errors: ${extras.length}${extras.length ? ` ${extras.join(", ")}` : ""}`);
}

await main();
