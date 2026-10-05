import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function codes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("aliases are expanded before requirement keys are validated", () => {
  assert.deepEqual(
    codes(`type MissingAlias = Missing
fn run() -> void $ MissingAlias: pass`),
    ["unknown-type", "unknown-trait"],
  );
});
