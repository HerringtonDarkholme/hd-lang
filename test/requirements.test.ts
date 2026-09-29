import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { isRowSubsumption } from "../src/checker/assignability.ts";
import { explainRequirements } from "../src/requirements.ts";
import { splitRowKeys } from "../src/types.ts";
import { fixture } from "./fixture.ts";

test("requirement explanations include transitive call paths", () => {
  const source = fixture("requirements/transitive-call-paths");
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  const explanations = explainRequirements(analysis.hir!);
  assert.deepEqual(explanations[2]?.paths, [
    { key: "Clock", path: ["main", "middle", "read", "$.use(Clock)"] },
  ]);
});

test("lexical overrides explain their actual outer provider dependency", () => {
  const source = fixture("requirements/lexical-override");
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.deepEqual(explainRequirements(analysis.hir!)[1]?.paths, [
    { key: "Backup", path: ["main", "$.use(Backup)"] },
  ]);
});

test("requirement explanations traverse closure providers and loop exits", () => {
  const closureSource = fixture("requirements/closure-provider");
  const closure = analyze(closureSource);
  assert.deepEqual(closure.diagnostics, []);
  assert.deepEqual(explainRequirements(closure.hir!)[0]?.paths, [
    { key: "Clock", path: ["main", "$.use(Clock)"] },
  ]);

  const loopSource = fixture("requirements/loop-exit");
  const loop = analyze(loopSource);
  assert.deepEqual(loop.diagnostics, []);
  assert.deepEqual(explainRequirements(loop.hir!)[1]?.paths, [
    { key: "Clock", path: ["main", "read", "declared"] },
  ]);
});

test("a row splits at top-level '+' only, keeping a row alias's row argument whole", () => {
  assert.deepEqual(splitRowKeys("Clock+WithLog[$(Db+Log)]"), ["Clock", "WithLog[$(Db+Log)]"]);
  assert.deepEqual(splitRowKeys(""), []);
});

test("row subsumption widens a function value's row but never solves a row parameter", () => {
  assert.equal(isRowSubsumption("fn()->i32$Clock", "fn()->i32$Clock+Db"), true);
  assert.equal(isRowSubsumption("fn()->i32", "fn()->i32$Db"), true);
  assert.equal(isRowSubsumption("fn()->i32$Metrics", "fn()->i32$Db"), false);
  assert.equal(isRowSubsumption("fn()->i32$Clock", "fn()->i32$Clock+row:R"), false);
  assert.equal(isRowSubsumption("List[fn()->i32$Clock]", "List[fn()->i32$Clock+Db]"), false);
});

test("a diagnostic under an aliased row names the missing key and the expansion", () => {
  const analysis = analyze(
    [
      "trait Db",
      "trait Metrics",
      "type WebRow = Db",
      "type AppRow = WebRow + Db",
      'fn respond() -> string $ Metrics: "ok"',
      "fn get_order() -> string $ AppRow:",
      "    respond()",
      "",
    ].join("\n"),
  );
  assert.deepEqual(
    analysis.diagnostics.map((diagnostic) => diagnostic.message),
    ["call to 'respond' requires Metrics; the row '$ AppRow' expands to '$ Db'"],
  );
});
