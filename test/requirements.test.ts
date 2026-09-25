import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { explainRequirements } from "../src/requirements.ts";

test("requirement explanations include transitive call paths", () => {
  const source = `fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    40
fn middle() -> i32 $ Clock: read() + 1
fn main() -> i32 $ Clock: middle() + 1
`;
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  const explanations = explainRequirements(analysis.hir!);
  assert.deepEqual(explanations[2]?.paths, [
    { key: "Clock", path: ["main", "middle", "read", "$.use(Clock)"] },
  ]);
});

test("lexical overrides explain their actual outer provider dependency", () => {
  const source = `fn read() -> i32 $ Clock: 42
fn main() -> i32 $ Backup:
    $.with(Clock=$.use(Backup)):
        read()
`;
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.deepEqual(explainRequirements(analysis.hir!)[1]?.paths, [
    { key: "Backup", path: ["main", "$.use(Backup)"] },
  ]);
});

test("requirement explanations traverse closure providers and loop exits", () => {
  const closureSource = `fn main() -> i32 $ Clock:
    callback := fn() -> i32 $ Clock:
        _ := $.use(Clock)
        42
    callback()
`;
  const closure = analyze(closureSource);
  assert.deepEqual(closure.diagnostics, []);
  assert.deepEqual(explainRequirements(closure.hir!)[0]?.paths, [
    { key: "Clock", path: ["main", "$.use(Clock)"] },
  ]);

  const loopSource = `fn read() -> i32 $ Clock: 42
fn main() -> i32 $ Clock:
    while true:
        break read()
    else:
        0
`;
  const loop = analyze(loopSource);
  assert.deepEqual(loop.diagnostics, []);
  assert.deepEqual(explainRequirements(loop.hir!)[1]?.paths, [
    { key: "Clock", path: ["main", "read", "declared"] },
  ]);
});
