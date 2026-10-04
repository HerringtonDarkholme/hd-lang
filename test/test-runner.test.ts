import assert from "node:assert/strict";
import test from "node:test";

import { compileToWasm, instantiate } from "../src/compiler.ts";
import { propertyRun } from "../src/property-tests.ts";
import { RuntimePanicError } from "../src/runtime-panic.ts";
import { runSelected } from "../src/test-runner.ts";

async function runTests(source: string, properties = propertyRun()) {
  const compilation = await compileToWasm(source);
  const selected = compilation.hir.functions.filter((declaration) => declaration.testOptions);
  const shared = await instantiate(source, { compilation });
  return runSelected(
    selected,
    shared.instance.exports,
    async () => (await instantiate(source, { compilation })).instance.exports,
    undefined,
    properties,
  );
}

test("explicit panics preserve their message at the runtime boundary", async () => {
  const { instance } = await instantiate('pub fn main() -> void: panic("case discarded")\n');
  assert.throws(
    () => (instance.exports.main as CallableFunction)(),
    (error: unknown) =>
      error instanceof RuntimePanicError &&
      error.code === "explicit-panic" &&
      error.detail === "case discarded",
  );
});

test("the test runner answers TestRunner.snapshot_check", async () => {
  const source = `use std.testing.TestRunner
tests:
    it("snapshot"):
        problem := $.use(TestRunner).snapshot_check("hello")
        if problem != "": panic(problem)
`;
  const compilation = await compileToWasm(source);
  const selected = compilation.hir.functions.filter((declaration) => declaration.testOptions);
  const shared = await instantiate(source, { compilation });
  const checked: string[] = [];
  const outcome = await runSelected(
    selected,
    shared.instance.exports,
    async () => (await instantiate(source, { compilation })).instance.exports,
    undefined,
    propertyRun(),
    (text) => {
      checked.push(text);
      return "";
    },
  );
  assert.deepEqual(checked, ["hello"]);
  assert.equal(outcome.kind, "passed");
});

test("the discard panic is a discard only before PropertyRunner.show", async () => {
  const beforeShow = `use std.testing.{Choices, it_prop_with}
fn rejected(c: mut Choices) -> i64:
    panic("std.testing: case discarded")
tests:
    it_prop_with("discard", cases=1, expect_panic="explicit-panic", gen=rejected, prop=fn!(value: i64): pass)
`;
  const discarded = await runTests(beforeShow, propertyRun({ cases: 1, seed: 1 }));
  assert.equal(discarded.kind, "failed");
  if (discarded.kind === "failed") assert.match(discarded.outcome ?? "", /discarded 11 cases/);

  const afterShow = `use std.testing.it_prop
tests:
    it_prop("body", cases=1, prop=fn!(value: i32):
        panic("std.testing: case discarded")
    )
`;
  const failed = await runTests(afterShow, propertyRun({ cases: 1, seed: 1 }));
  assert.equal(failed.kind, "failed");
  if (failed.kind === "failed")
    assert.match(
      failed.outcome ?? "",
      /panicked with explicit-panic: std\.testing: case discarded/,
    );
});

test("the property runner returns a structural case and records choices", () => {
  const run = propertyRun({ seed: 41 });
  run.start([3n, 5n], 43, 7, true);
  assert.deepEqual(run.answer("start", [10, 20, 0]), {
    value: {
      example: { tag: "none" },
      seed: 43n,
      size: 7,
      replay: [3n, 5n],
    },
  });
  run.answer("record", [3n]);
  run.answer("record", [5n]);
  assert.deepEqual(run.recorded(), [3n, 5n]);

  run.startExample(0);
  assert.deepEqual(run.answer("start", [10, 20, 1]), {
    value: {
      example: { tag: "some", value: 0 },
      seed: 41n,
      size: 0,
      replay: [],
    },
  });
});
