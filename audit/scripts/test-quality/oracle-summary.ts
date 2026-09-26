// Builds oracle.tsv from oracle-runs.tsv plus static checks (c)-(e) (audit step 1.6).
// Usage: node --experimental-strip-types audit/scripts/test-quality/oracle-summary.ts
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { type FixtureInfo, header, readConformance, readFixtures, root, tsv } from "./lib.ts";

const out = resolve(root, "audit/evidence/01-test-quality");

// Manual corrections after inspecting each copy (see SUMMARY.md, "Oracle strength").
const overrides = new Map<string, string>([
  ["parse/invalid/tab-indentation.hd", "isolated-accepts (rerun: pass indented with spaces; first copy kept the tab)"],
  ["frontend/13-tabs-and-inconsistent-dedents-are-rejected-by-the-lexer.hd", "isolated-other-errors:missing-return-value (rerun with spaces)"],
  ["compiler/62-nonrecursive-closures-infer-result-types-from-fallthrough-and-returns-closure-result-type.hd", "code-persists-legitimately (void fallthrough still mixes with i32)"],
  ["runtime/panic/competing-suspension-drivers.hd", "code-persists (scenario harness panics regardless of the marked line)"],
  ["runtime/panic/reentrant-suspension-poll.hd", "code-persists (scenario harness panics regardless of the marked line)"],
  ["compiler/46-explicit-panic-lowers-to-unreachable-and-skips-pending-defer.hd", "code-persists (panic comes from line 6 `panic(\"boom\")`; marker sits on the defer body that must not run)"],
]);

function observation(fixture: FixtureInfo): string {
  const text = fixture.lines.filter((line) => !line.startsWith("#")).join("\n");
  if (fixture.results.length) {
    const trivial = fixture.results.every(({ entry, value }) =>
      new RegExp(`^fn ${entry}!?\\(\\)\\s*->\\s*\\w+\\s*:\\s*${value.replace("-", "\\-")}\\s*$`, "m").test(text),
    );
    return trivial ? "result-constant-literal" : "result";
  }
  if (fixture.scenario === "cancellation-cleanup") return "cleanup_ran-check";
  if (/\bassert(_equal)?\(/.test(text)) return "assert";
  if (/\bpanic\(/.test(text)) return "panic-guard";
  return "none";
}

function isRuntimeAccept(fixture: FixtureInfo): boolean {
  if (fixture.body === "spec/conformance") return fixture.phase === "runtime" && fixture.expectation === "accept";
  return fixture.markers.length === 0 && (fixture.results.length > 0 || fixture.expects.includes("test"));
}

function groupKey(fixture: FixtureInfo): string {
  if (fixture.body === "spec/conformance") return fixture.section ?? "";
  const numbered = /^((?:compiler|frontend|suspension)\/\d\d)-/.exec(fixture.path);
  return numbered ? numbered[1]! : fixture.path.replace(/\/[^/]*$/, "");
}

function tokens(fixture: FixtureInfo): Set<string> {
  const text = fixture.lines.filter((line) => !line.startsWith("#")).map((line) => line.replace(/#.*$/, "")).join("\n");
  return new Set(text.match(/[A-Za-z_][A-Za-z0-9_]*|\d+|[^\sA-Za-z0-9_]/g) ?? []);
}

function jaccard(left: Set<string>, right: Set<string>): number {
  let shared = 0;
  for (const token of left) if (right.has(token)) shared += 1;
  return shared / (left.size + right.size - shared || 1);
}

function andChain(text: string): number {
  let max = 0;
  for (const line of text.split("\n")) max = Math.max(max, (line.match(/\band\b/g) ?? []).length);
  return max;
}

function main(): void {
  const runs = new Map<string, string[]>();
  for (const row of readFileSync(resolve(out, "oracle-runs.tsv"), "utf8").trimEnd().split("\n").slice(2)) {
    const cells = row.split("\t");
    runs.set(`${cells[0]}\t${cells[1]}`, cells);
  }
  const all = [...readConformance(), ...readFixtures()];
  const accepting = new Map<string, number>();
  for (const fixture of all)
    if (fixture.markers.length === 0) accepting.set(`${fixture.body}\t${groupKey(fixture)}`, (accepting.get(`${fixture.body}\t${groupKey(fixture)}`) ?? 0) + 1);
  const names = new Map<string, number>();
  for (const fixture of all) if (fixture.name) names.set(fixture.name, (names.get(fixture.name) ?? 0) + 1);
  const head = [
    "body", "case", "expectation", "selection", "a_neutralized_verdict", "b_marker_observed", "b_extra_errors",
    "c_runtime_observation", "d_accept_twin_in_group", "e_scope_flags",
  ];
  const rows: unknown[][] = [];
  for (const fixture of all) {
    const run = runs.get(`${fixture.body}\t${fixture.path}`);
    const text = fixture.lines.filter((line) => !line.startsWith("#")).join("\n");
    const scope: string[] = [];
    if (fixture.markers.length > 1) scope.push(`markers=${fixture.markers.length}`);
    if (fixture.results.length > 2) scope.push(`results=${fixture.results.length}`);
    const chain = andChain(text);
    if (chain >= 3) scope.push(`and-chain=${chain}`);
    if (fixture.name && (names.get(fixture.name) ?? 0) > 1) scope.push(`shared-test-name=${names.get(fixture.name)}`);
    if (fixture.markers[0]?.kind === "panic" && /^\s*(pub\s+)?fn\b/.test(fixture.lines[fixture.markers[0].line - 1] ?? ""))
      scope.push("panic-marker-on-fn-header");
    const verdict = overrides.get(fixture.path) ?? run?.[12] ?? "";
    let twin = "";
    if (fixture.markers.length) {
      const own = tokens(fixture);
      let best = 0;
      let bestPath = "";
      for (const other of all)
        if (other.body === fixture.body && other.markers.length === 0) {
          const score = jaccard(own, tokens(other));
          if (score > best) [best, bestPath] = [score, other.path];
        }
      const group = (accepting.get(`${fixture.body}\t${groupKey(fixture)}`) ?? 0) > 0 ? "group-accept" : "no-group-accept";
      twin = `${best >= 0.5 ? "near" : "far"} ${best.toFixed(2)} ${bestPath} ${group}`;
    }
    rows.push([
      fixture.body,
      fixture.path,
      fixture.body === "spec/conformance" ? `${fixture.phase}:${fixture.expectation}` : (fixture.markers.map((m) => `${m.kind}:${m.code}`).join(",") || (fixture.results.length ? "result" : `expect:${fixture.expects.join(",")}`)),
      fixture.body === "spec/conformance" ? (fixture.selected ? "selected" : "unselected") : "run-portable",
      verdict,
      run?.[6] ?? "",
      run?.[7] === "yes" ? run[8] : run?.[7] ?? "",
      isRuntimeAccept(fixture) ? observation(fixture) : "",
      twin,
      scope.join(" "),
    ]);
  }
  writeFileSync(resolve(out, "oracle.tsv"), `${header("node --experimental-strip-types audit/scripts/test-quality/oracle-summary.ts (after oracle.ts)")}\n${tsv([head, ...rows])}\n`);
  const count = (predicate: (row: unknown[]) => boolean): number => rows.filter(predicate).length;
  const stats = {
    marked: count((r) => r[4] !== ""),
    persists: count((r) => String(r[4]).startsWith("code-persists")),
    extraErrors: count((r) => r[6] !== "" && r[6] !== "no" && r[6] !== "n/a"),
    runtimeAccept: count((r) => r[7] !== ""),
    runtimeUnobserved: count((r) => r[7] === "none"),
    runtimeConstant: count((r) => r[7] === "result-constant-literal"),
    rejectNoNearTwin: count((r) => String(r[8]).startsWith("far")),
    rejectNoNearTwinConformance: count((r) => String(r[8]).startsWith("far") && r[0] === "spec/conformance"),
    rejectNoNearTwinFixtures: count((r) => String(r[8]).startsWith("far") && r[0] === "test/fixtures"),
    rejectNoGroupAccept: count((r) => String(r[8]).endsWith("no-group-accept")),
    multiMarker: count((r) => /markers=/.test(String(r[9]))),
    sharedName: count((r) => /shared-test-name/.test(String(r[9]))),
    andChain: count((r) => /and-chain/.test(String(r[9]))),
    manyResults: count((r) => /results=/.test(String(r[9]))),
  };
  console.log(JSON.stringify(stats));
}

main();
