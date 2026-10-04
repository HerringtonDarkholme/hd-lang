import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { isRowSubsumption } from "../src/checker/assignability.ts";
import { splitRowKeys } from "../src/types.ts";

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
      "type AppRow = $ WebRow + Db",
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

test("a row alias's right side without '$' gets a fix-it that adds it", () => {
  const source = [
    "trait Db",
    "trait Cache",
    "type AppRow = Db + Cache",
    "type Web = AppRow",
    "",
  ].join("\n");
  const diagnostics = analyze(source).diagnostics;
  assert.deepEqual(
    diagnostics.map((diagnostic) => diagnostic.code),
    ["generic-kind-mismatch", "generic-kind-mismatch"],
  );
  const fixed = diagnostics
    .map((diagnostic) => diagnostic.fix!.edits[0]!)
    .sort((left, right) => right.span.start.offset - left.span.start.offset)
    .reduce(
      (text, edit) =>
        text.slice(0, edit.span.start.offset) + edit.replacement + text.slice(edit.span.end.offset),
      source,
    );
  assert.equal(fixed, source.replace("= Db", "= $ Db").replace("= AppRow", "= $ AppRow"));
  assert.deepEqual(analyze(fixed).diagnostics, []);
});

test("a script top level infers its entry requirement row (module.init.script-row)", () => {
  const source = ['greeting := "hello"', "println(greeting)", 'println("done")', ""].join("\n");
  assert.deepEqual(analyze(source).diagnostics, []);
});
