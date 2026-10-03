import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

const producer = "data Box[+T]:\n    value: T\n";
const consumer = "data Consumer[-T]:\n    consume: fn(T) -> void\n";

function diagnostics(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("readonly public method inputs participate in nominal variance", () => {
  const source = `${producer}
impl[U] Box[U]:
    pub fn consume(self, value: U) -> void:
        ()
`;
  const result = analyze(source).diagnostics;
  assert.deepEqual(
    result.map((diagnostic) => diagnostic.code),
    ["invalid-variance"],
  );
  assert.equal(result[0]?.message, "'+U' occurs in a negative position");
  assert.equal(result[0]?.span.start.line, 5);
});

test("readonly public method results participate in nominal variance", () => {
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    pub fn produce(self, factory: fn() -> U) -> U:
        factory()
`),
    ["invalid-variance"],
  );
});

test("nested function parameter polarity reverses rather than becoming invariant", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn visit(self, visitor: fn(U) -> void) -> void:
        visitor(self.value)
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn produce(self, factory: fn() -> U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("mutable method signature positions are invariant", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume(self, value: mut U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("invariant method signature containers cannot hide variance violations", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume(self, value: U?) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("method generic binders do not capture implementation parameters in Self", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T:
        value
    pub fn copy[T](self) -> Self:
        self
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn consume[T](self, other: Self) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("nested implementation targets compose their declared variance signs", () => {
  assert.deepEqual(
    diagnostics(`${producer}
${consumer}
impl[U] Box[Consumer[U]]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
${consumer}
impl[U] Box[Consumer[U]]:
    pub fn produce(self, factory: fn() -> U) -> U:
        factory()
`),
    ["invalid-variance"],
  );
});

test("an invariant implementation target does not impose signed method constraints", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U?]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("opposing target signs constrain a shared parameter to equality", () => {
  assert.deepEqual(
    diagnostics(`data Pair[+A, -B]:
    value: A
    consume: fn(B) -> void

impl[U] Pair[U, U]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("trait arguments in method signatures retain their invariant polarity", () => {
  assert.deepEqual(
    diagnostics(`${producer}
trait Sink[T]:
    fn consume(self, value: T) -> void

impl[U] Box[U]:
    pub fn accept(self, sink: Sink[U]) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("a separate trait implementation does not change nominal variance", () => {
  assert.deepEqual(
    diagnostics(`${producer}
trait Sink[T]:
    fn consume(self, value: T) -> void

impl[U] Sink[U] for Box[U]:
    fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("enum inherent methods are included in the readonly public surface", () => {
  assert.deepEqual(
    diagnostics(`enum Choice[+T]:
    Some(value: T)

impl[U] Choice[U]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("suspending inherent methods have the same variance obligations", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume!(self, value: U) -> void:
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

test("a public method cannot infer its result past the variance check", () => {
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    pub fn produce(self, factory: fn() -> U):
        factory()
`),
    ["missing-result-type"],
  );
});
