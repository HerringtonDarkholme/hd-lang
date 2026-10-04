import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

test("a module namespace does not expose a private standard function", () => {
  const diagnostics = analyze(`use std.text

fn main():
    text.hex_digit
`).diagnostics;
  assert.equal(diagnostics.length, 1);
  assert.equal(diagnostics[0]?.code, "unknown-name");
});
