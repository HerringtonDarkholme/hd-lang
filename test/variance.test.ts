import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

const producer = "data Box[+T]:\n    value: T\n";
const consumer = "data Consumer[-T]:\n    consume: fn(T) -> void\n";

function diagnostics(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("method generic binders and Self keep the implementation's parameter", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn echo[U](self, value: U) -> U:
        value
    pub fn copy[U](self) -> Self:
        self
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn consume[U](self, other: Self) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("associated construction and mutable receiver signatures are not readonly instance views", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn new(value: U) -> Self:
        Box::[U] { value: value }
    pub fn set(mut self, value: U) -> void:
        self.value = value
`),
    [],
  );
});

test("method binders cannot erase Self-derived invariant inferred results", () => {
  for (const binder of ["U", "V"]) {
    for (const body of [
      "Wrap { value: self.consume }",
      "let mut values = [self.consume]\n        values",
    ]) {
      const result = analyze(`data Wrap[T]:
    value: T
${consumer}
impl[T] Consumer[T]:
    fn hidden[${binder}](self):
        ${body}
`).diagnostics;
      assert.deepEqual(
        result.map((d) => d.code),
        ["invalid-variance"],
      );
      assert.equal(result[0]!.message, "'-T' occurs in an invariant position");
    }
  }
});

test("method-owned inferred and callable results are independent of the receiver binder", () => {
  for (const visibility of ["", "pub "]) {
    assert.deepEqual(
      diagnostics(`${producer}
impl[T] Box[T]:
    ${visibility}fn echo[U](self, value: U) -> U: value
    ${visibility}fn callback[U](self, value: U) -> fn() -> U:
        fn() -> U: value
`),
      [],
    );
  }
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    fn echo[U](self, value: U): value
    fn callback[U](self, value: U):
        fn() -> U: value
    fn receiver[U](self): self
`),
    [],
  );
});
