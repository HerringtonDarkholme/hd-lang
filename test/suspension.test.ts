import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

test("cleanup cannot propagate out of an ordinary or suspending function", () => {
  for (const name of ["run", "run!"]) {
    const source = `fn ${name}() -> Result[i32, string]:
    defer:
        let answer: Result[i32, string] = .Err("failure")
        _ := answer?
    .Ok(42)
`;
    assert.equal(analyze(source).diagnostics[0]?.code, "defer-control-flow");
  }
});

test("a closure declared in cleanup can propagate within its own function", async () => {
  const source = `fn main() -> i32:
    defer:
        inner := fn() -> Result[i32, string]:
            let answer: Result[i32, string] = .Err("failure")
            _ := answer?
            .Ok(0)
        _ := inner()
    42
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
