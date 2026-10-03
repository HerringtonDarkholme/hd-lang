import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";

async function runMain(source: string): Promise<unknown> {
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  return (instance.exports.main as CallableFunction)();
}

test("a standard module member is a function value", async () => {
  const source = `use std.text
use std.ops.Template

fn keep(prefix: fn(Template[Display]) -> string) -> i32: 42

fn main() -> i32:
    keep(text.r)
`;
  assert.equal(await runMain(source), 42);
});

test("a renamed standard module resolves member values", async () => {
  const source = `use std.text as raw
use std.ops.Template

fn keep(prefix: fn(Template[Display]) -> string) -> i32: 42

fn main() -> i32:
    keep(raw.r)
`;
  assert.equal(await runMain(source), 42);
});

test("an expected function type instantiates a generic module member", async () => {
  const source = `use std.cmp

fn choose(comparison: fn(i32, i32) -> i32) -> i32:
    comparison(42, 50)

fn main() -> i32:
    choose(cmp.min)
`;
  assert.equal(await runMain(source), 42);
});

test("explicit type arguments instantiate a generic module member value", async () => {
  const source = `use std.cmp

fn choose(comparison: fn(i32, i32) -> i32) -> i32:
    comparison(42, 50)

fn main() -> i32:
    choose(cmp.min::[i32])
`;
  assert.equal(await runMain(source), 42);
});

test("a parenthesized module member remains an ordinary function value", async () => {
  const source = `use std.text

fn main() -> i32:
    if (text.join)(["left", "right"], ":") == "left:right": 42 else: 0
`;
  assert.equal(await runMain(source), 42);
});

test("module selection finds a function bound through the prelude", async () => {
  const source = `use std.format

fn render(value: fn(i32) -> string) -> i32: 42

fn main() -> i32:
    render(format.debug)
`;
  assert.equal(await runMain(source), 42);
});

test("module selection finds the same directly imported declaration", async () => {
  const source = `use std.cmp
use std.cmp.max

fn choose(comparison: fn(i32, i32) -> i32) -> i32:
    comparison(42, 50)

fn main() -> i32:
    choose(cmp.max)
`;
  assert.equal(await runMain(source), 50);
});

test("module selection finds the same declaration imported under an alias", async () => {
  const source = `use std.cmp
use std.cmp.max as maximum

fn choose(comparison: fn(i32, i32) -> i32) -> i32:
    comparison(42, 50)

fn main() -> i32:
    choose(cmp.max)
`;
  assert.equal(await runMain(source), 50);
});

test("a local value shadows a standard module namespace", async () => {
  const source = `use std.cmp

data Choice:
    min: i32

fn main() -> i32:
    choice := Choice { min: 42 }
    let cmp = choice
    cmp.min
`;
  assert.equal(await runMain(source), 42);
});

test("a module namespace does not expose a private standard function", () => {
  const diagnostics = analyze(`use std.text

fn main():
    text.hex_digit
`).diagnostics;
  assert.equal(diagnostics.length, 1);
  assert.equal(diagnostics[0]?.code, "unknown-name");
});
