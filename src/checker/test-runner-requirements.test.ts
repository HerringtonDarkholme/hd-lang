import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

const prelude = `use std.testing.TestRunner

fn use_runner() -> void $ TestRunner:
    _ := $.use(TestRunner).row(1)
`;

test("a plain test body receives the TestRunner capability", () => {
  const source = `${prelude}
tests:
    it("uses runner"):
        use_runner()
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const declaration = result.hir!.functions.find((item) => item.testOptions);
  assert.deepEqual(declaration?.requirements, ["TestRunner"]);
});

for (const [name, source] of [
  [
    "timed",
    `${prelude}
use std.time.s
tests:
    it("uses runner", timeout=1s):
        use_runner()
`,
  ],
  [
    "table",
    `${prelude}
use std.testing.it_each
tests:
    it_each("uses runner", [1], body=fn!(value: i32):
        use_runner()
    )
`,
  ],
  [
    "property",
    `${prelude}
use std.testing.it_prop
tests:
    it_prop("uses runner", prop=fn!(value: i32):
        use_runner()
    )
`,
  ],
  [
    "property with a generator",
    `${prelude}
use std.testing.{Choices, it_prop_with}

fn one(choices: mut Choices) -> i32: 1

tests:
    it_prop_with("uses runner", gen=one, prop=fn!(value: i32):
        use_runner()
    )
`,
  ],
] as const) {
  test(`a ${name} test body receives the TestRunner capability`, () => {
    const result = analyze(source);
    assert.deepEqual(result.diagnostics, []);
  });
}

test("a unit test body does not receive PropertyRunner", () => {
  const result = analyze(`use std.testing.PropertyRunner

fn use_property_runner() -> void $ PropertyRunner:
    $.use(PropertyRunner).show("input")

tests:
    it("does not receive the property runner"):
        use_property_runner()
`);
  assert.deepEqual(
    result.diagnostics.map((diagnostic) => diagnostic.code),
    ["missing-requirement"],
  );
  assert.equal(result.diagnostics[0]!.span.start.line, 8);
});
