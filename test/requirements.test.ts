import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { explainRequirements } from "../src/requirements.ts";
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
