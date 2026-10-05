import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

// The private case is the fixture typing/invalid/module-path-private-std-function.hd.
test("a module path to a missing standard declaration is unknown-import", () => {
  const diagnostics = analyze(`use std.text

fn main() -> string:
    text.joinn(["a"], "")
`).diagnostics;
  assert.deepEqual(
    diagnostics.map(({ code }) => code),
    ["unknown-import"],
  );
});
