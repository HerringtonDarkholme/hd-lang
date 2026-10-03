import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

test("every test body receives the TestRunner capability", () => {
  const source = `use std.testing.TestRunner
tests:
    it("uses runner"):
        _ := $.use(TestRunner).row(1)
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const declaration = result.hir!.functions.find((item) => item.testOptions);
  assert.deepEqual(declaration?.requirements, ["TestRunner"]);
});
