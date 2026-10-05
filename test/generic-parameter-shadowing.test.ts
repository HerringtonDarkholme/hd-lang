import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

function userTypeProgram(name: string): string {
  return [
    `data ${name}:`,
    `    value: i32`,
    ``,
    `pub fn probe() -> i32:`,
    `    ${name} { value: +41 }.value + +1`,
    ``,
    `pub fn main() -> void:`,
    `    pass`,
    ``,
  ].join("\n");
}

for (const name of ["T", "E", "K", "V"]) {
  test(`a user type named ${name} compiles and runs beside std generics`, async () => {
    const source = userTypeProgram(name);
    assert.deepEqual(analyze(source).diagnostics, []);
    const { instance } = await instantiate(source);
    assert.equal((instance.exports.probe as CallableFunction)(), 42);
  });
}

test("a generic parameter shadows a same-named user type", async () => {
  const source = [
    `data T:`,
    `    value: i32`,
    ``,
    `fn id[T](x: T) -> T:`,
    `    x`,
    ``,
    `pub fn probe() -> i32:`,
    `    id(T { value: +7 }).value`,
    ``,
    `pub fn main() -> void:`,
    `    pass`,
    ``,
  ].join("\n");
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const id = result.hir?.functions.find((fn) => fn.name === "id");
  assert.equal(id?.parameters[0]?.type, "generic:T");
  assert.equal(id?.result, "generic:T");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.probe as CallableFunction)(), 7);
});

test("a std generic runs with a same-named user type in scope", async () => {
  const source = [
    `use std.cmp.min`,
    ``,
    `data T:`,
    `    value: i32`,
    ``,
    `pub fn probe() -> i32:`,
    `    T { value: +1 }.value + min(+3, +5)`,
    ``,
    `pub fn main() -> void:`,
    `    pass`,
    ``,
  ].join("\n");
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.probe as CallableFunction)(), 4);
});

test("a genuinely private type in a public signature still leaks", () => {
  const leakedResult = [
    `data Secret:`,
    `    value: string`,
    ``,
    `pub fn reveal() -> Secret:`,
    `    Secret { value: "hidden" }`,
    ``,
  ].join("\n");
  assert.deepEqual(
    analyze(leakedResult).diagnostics.map((diagnostic) => diagnostic.code),
    ["private-type-leak"],
  );
  const leakedRequirement = [
    `trait Database`,
    ``,
    `pub fn query() -> void $ Database:`,
    `    pass`,
    ``,
  ].join("\n");
  assert.deepEqual(
    analyze(leakedRequirement).diagnostics.map((diagnostic) => diagnostic.code),
    ["private-type-leak"],
  );
});

test("no declaration in a type parameter's scope reuses its name", () => {
  const cases = [
    "impl[T] Box[T]:\n    fn echo[T](self, value: T) -> T:\n        value\n",
    "trait Maker[T]:\n    fn make[T](self) -> T\n",
    "fn work[T](value: T) -> T:\n    data T:\n        count: i32\n    value\n",
    "fn work[T](value: T) -> T:\n    data Inner[T]:\n        item: T\n    value\n",
    "fn work[T](value: T) -> T:\n    T := value\n    value\n",
    "fn work[T](T: i32, value: T) -> T:\n    value\n",
    "fn work[T](values: List[T]) -> i32:\n    for T in values:\n        pass\n    +0\n",
    "fn work[T](value: T?) -> i32:\n    match value:\n        .Some(T) => +1\n        .None => +0\n",
    "fn work[T](value: T) -> fn(i32) -> i32:\n    fn(T: i32) -> i32: T\n",
  ];
  for (const body of cases) {
    const source = `data Box[T]:\n    value: T\n\n${body}`;
    assert.deepEqual(
      analyze(source).diagnostics.map((diagnostic) => diagnostic.code),
      ["duplicate-binding"],
      body,
    );
  }
});
