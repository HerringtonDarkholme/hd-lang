import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function codes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

const unsafe = `trait Factory:
    fn create() -> Self
`;

test("local annotations validate direct and nested trait values", () => {
  assert.deepEqual(
    codes(`${unsafe}
fn direct() -> void:
    let direct: Factory = panic("stop")

fn nested() -> void:
    let nested: List[Factory] = []
`),
    ["trait-not-dynamically-safe", "trait-not-dynamically-safe"],
  );
});

test("signature types validate every nested value position", () => {
  assert.deepEqual(
    codes(`${unsafe}
fn use(
    values: List[Factory],
    pair: (i32, Factory?),
    callback: fn(Factory) -> Result[i32, Factory],
) -> void:
    pass
`),
    ["trait-not-dynamically-safe", "trait-not-dynamically-safe", "trait-not-dynamically-safe"],
  );
});

test("data and enum fields validate after every trait body is defined", () => {
  assert.deepEqual(
    codes(`data Box:
    value: List[Factory]

enum Event:
    Value(Factory?)

${unsafe}`),
    ["trait-not-dynamically-safe", "trait-not-dynamically-safe"],
  );
});

test("trait method types are validated once after trait definition", () => {
  assert.deepEqual(
    codes(`${unsafe}
trait Consumer:
    fn consume(self, values: List[Factory]) -> void
`),
    ["trait-not-dynamically-safe"],
  );
});

test("associated binding values are nested value types", () => {
  assert.deepEqual(
    codes(`trait Store:
    type Item
    fn load(self) -> Self::Item

${unsafe}
fn use(value: Store[Item = Factory]) -> void:
    pass
`),
    ["trait-not-dynamically-safe"],
  );
});

test("function bounds and requirement keys validate their nested value types", () => {
  assert.deepEqual(
    codes(`trait Source[T]
trait Store:
    type Item
    fn load(self) -> Self::Item

${unsafe}
fn by_argument[T < Source[List[Factory]]](value: T) -> void: pass
fn by_binding[T < Store[Item = Factory]](value: T) -> void: pass
fn by_requirement() -> void $ Source[Factory]: pass
`),
    ["trait-not-dynamically-safe", "trait-not-dynamically-safe", "trait-not-dynamically-safe"],
  );
});

test("declaration bounds validate after every trait body is defined", () => {
  assert.deepEqual(
    codes(`data Box[T < Source[Factory]]:
    value: T

enum Choice[T < Source[Factory]]:
    Value(T)

trait Consumer[T < Source[Factory]]:
    fn consume[U < Source[Factory]](self, value: U) -> void

trait Source[T]
${unsafe}`),
    [
      "trait-not-dynamically-safe",
      "trait-not-dynamically-safe",
      "trait-not-dynamically-safe",
      "trait-not-dynamically-safe",
    ],
  );
});

test("an implementation bound is validated even when the impl has no methods", () => {
  assert.deepEqual(
    codes(`trait Source[T]
trait Marker

data Wrap[T]:
    value: T

impl[T < Source[Factory]] Marker for Wrap[T]

${unsafe}`),
    ["trait-not-dynamically-safe"],
  );
});

test("lowered alias and newtype bounds retain their written value types", () => {
  assert.deepEqual(
    codes(`trait Source[T]

type Alias[T < Source[Factory]] = T
type Wrap[T < Source[Factory]](T)

${unsafe}`),
    ["trait-not-dynamically-safe", "trait-not-dynamically-safe"],
  );
});

test("nested dynamically safe traits remain valid", () => {
  assert.deepEqual(
    codes(`trait Visitor:
    fn visit[T < AnyRef](self, value: T) -> void

data Work:
    visitors: List[Visitor]

fn use(callback: fn(Visitor?) -> List[Visitor]) -> void:
    let current: Visitor? = .None
    _ := current
`),
    [],
  );
});
