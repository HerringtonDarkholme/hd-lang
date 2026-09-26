// 5.7 MVP goal 2: wall time of the edit-compile-validate-run loop per fixture.
// For each selected conformance case and every test/fixtures case, time one CLI
// process that goes as far as the fixture allows (run/test for runnable cases,
// check for diagnostic cases, parse for parse cases), plus the in-process
// compile phases (parse, check, emit, assemble+validate) and instantiation.
// Run: node --experimental-strip-types audit/scripts/arch/bench-fixtures.ts
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";

import { compilePhased, header, median } from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const evidenceDir = resolve(root, "audit/evidence/05-requirements");

interface Case {
  readonly path: string;
  readonly args: readonly string[];
  readonly profile?: string;
}

function walk(directory: string): string[] {
  return readdirSync(directory)
    .flatMap((name) => {
      const path = resolve(directory, name);
      return statSync(path).isDirectory() ? walk(path) : name.endsWith(".hd") ? [path] : [];
    })
    .sort();
}

function directive(source: string, pattern: RegExp): string | undefined {
  return pattern.exec(source)?.[1];
}

function conformanceCases(): Case[] {
  const rows = readFileSync(resolve(root, "test/portable/cases.tsv"), "utf8").trimEnd().split("\n").slice(1);
  return rows.map((row) => {
    const [path, phase] = row.split("\t") as [string, string];
    const full = resolve(root, "spec/conformance", path);
    const source = readFileSync(full, "utf8");
    const profile = directive(source, /^# fixture-runtime-profile: ([a-z0-9-]+)/m);
    const scenario = directive(source, /^# fixture-runtime-scenario: ([a-z0-9-]+)/m);
    const pending = directive(source, /^# fixture-runtime-pending-function: ([A-Za-z0-9_]+)/m);
    const profileArgs = profile ? ["--profile", profile] : [];
    if (phase === "runtime")
      return {
        path: full,
        profile,
        args: [
          "test",
          ...(scenario ? ["--scenario", scenario] : []),
          ...(pending ? ["--pending-function", pending] : []),
          ...profileArgs,
        ],
      };
    return { path: full, args: [phase === "parse" ? "parse" : "check"] };
  });
}

function fixtureCases(): Case[] {
  return walk(resolve(root, "test/fixtures")).map((path) => {
    const source = readFileSync(path, "utf8");
    const profile = directive(source, /^# fixture-runtime-profile: ([a-z0-9-]+)/m);
    const profileArgs = profile ? ["--profile", profile] : [];
    const result = directive(source, /^# expect-result: ([A-Za-z0-9_]+) = /m);
    const expect = directive(source, /^# expect: ([a-z]+)/m);
    if (result) return { path, profile, args: ["run", "--entry", result, ...profileArgs] };
    if (/# panic: /.test(source)) return { path, profile, args: ["run", ...profileArgs] };
    if (expect === "test") return { path, profile, args: ["test", ...profileArgs] };
    if (expect === "parse") return { path, args: ["parse"] };
    return { path, profile, args: ["check", ...profileArgs] };
  });
}

const cases = [...conformanceCases(), ...fixtureCases()];
const rows: string[] = ["path\taction\texit\twall_ms\tparse_ms\tcheck_ms\temit_ms\tassemble_ms\tinstantiate_ms"];
const walls: number[] = [];
const inProcess: number[] = [];
const byAction = new Map<string, number[]>();
for (const testCase of cases) {
  const start = performance.now();
  const child = spawnSync(
    process.execPath,
    ["--experimental-strip-types", "bin/hd.js", ...testCase.args, testCase.path],
    { cwd: root, encoding: "utf8", timeout: 30_000 },
  );
  const wall = performance.now() - start;
  walls.push(wall);
  const action = testCase.args[0]!;
  byAction.set(action, [...(byAction.get(action) ?? []), wall]);
  let phases = ["", "", "", "", ""];
  try {
    const source = readFileSync(testCase.path, "utf8");
    const hostCapabilities = testCase.profile
      ? { "pending-gate": ["Gate"], "ready-counter": ["Counter"], "ready-float": ["FloatCell"], "ready-gate": ["Gate"], "ready-text": ["TextBridge"] }[testCase.profile]
      : undefined;
    const compiled = compilePhased(source, { hostCapabilities });
    const t0 = performance.now();
    await WebAssembly.compile(compiled.bytes);
    const instantiateMs = performance.now() - t0;
    inProcess.push(compiled.times.total + instantiateMs);
    const t = compiled.times;
    phases = [t.parse, t.check, t.emit, t.assemble, instantiateMs].map((value) => value.toFixed(1));
  } catch {
    // Diagnostic fixtures stop before emission.
  }
  rows.push([relative(root, testCase.path), testCase.args.join(" "), String(child.status), wall.toFixed(0), ...phases].join("\t"));
}
writeFileSync(resolve(evidenceDir, "fixture-loop.tsv"), rows.join("\n") + "\n");

function quantile(values: readonly number[], q: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;
}
const summary = [
  header("node --experimental-strip-types audit/scripts/arch/bench-fixtures.ts"),
  `Cases: ${cases.length} (${conformanceCases().length} selected conformance + ${fixtureCases().length} test/fixtures). Sequential; CPU shared with other audit workers.`,
  "",
  "| measure | n | median ms | p90 ms | p99 ms | max ms | over 1000 ms |",
  "| --- | --- | --- | --- | --- | --- | --- |",
  `| CLI wall (all) | ${walls.length} | ${median(walls).toFixed(0)} | ${quantile(walls, 0.9).toFixed(0)} | ${quantile(walls, 0.99).toFixed(0)} | ${Math.max(...walls).toFixed(0)} | ${walls.filter((w) => w > 1000).length} |`,
  ...[...byAction].map(
    ([action, values]) =>
      `| CLI wall (${action}) | ${values.length} | ${median(values).toFixed(0)} | ${quantile(values, 0.9).toFixed(0)} | ${quantile(values, 0.99).toFixed(0)} | ${Math.max(...values).toFixed(0)} | ${values.filter((w) => w > 1000).length} |`,
  ),
  `| in-process compile+validate+Wasm compile (emitting cases) | ${inProcess.length} | ${median(inProcess).toFixed(1)} | ${quantile(inProcess, 0.9).toFixed(1)} | ${quantile(inProcess, 0.99).toFixed(1)} | ${Math.max(...inProcess).toFixed(1)} | ${inProcess.filter((w) => w > 1000).length} |`,
  "",
  "Slowest 8 CLI invocations:",
  "",
  ...rows
    .slice(1)
    .sort((a, b) => Number(b.split("\t")[3]) - Number(a.split("\t")[3]))
    .slice(0, 8)
    .map((row) => `- ${row.split("\t").slice(0, 4).join(" | ")}`),
];
writeFileSync(resolve(evidenceDir, "fixture-loop.md"), summary.join("\n") + "\n");
console.log(summary.join("\n"));
