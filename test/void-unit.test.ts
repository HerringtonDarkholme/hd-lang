import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

// `void` is the empty tuple `()` (spec/lang/04-type-system.md#r-types.void).

function codes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("a void value is stored, passed, and matched, and each void expression runs once", async () => {
  const source = `data Counter:
    hits: i32

fn hit(counter: mut Counter) -> void:
    counter.hits = counter.hits + 1

data Marker:
    unit: void

fn take(unit: void) -> i32:
    +1

fn keep[T](value: T) -> T:
    value

pub fn probe() -> i32:
    let mut counter = Counter { hits: +0 }
    marker := Marker { unit: hit(counter) }
    let (unit, ten) = (hit(counter), +10)
    kept := keep(hit(counter))
    one := take(hit(counter))
    stored := hit(counter)
    let result: Result[void, string] = .Ok(hit(counter))
    matched := match result:
        .Ok(value) => +100
        .Err(_) => +0
    let same: () = stored
    counter.hits + ten + one + matched

pub fn main() -> void:
    pass
`;
  assert.deepEqual(
    analyze(source).diagnostics.filter((diagnostic) => diagnostic.severity !== "warning"),
    [],
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.probe as CallableFunction)(), 6 + 10 + 1 + 100);
});

test("void and () are one type, and a void value is Any", () => {
  assert.deepEqual(
    codes(`fn log() -> void:
    pass

fn same() -> ():
    log()

fn pair() -> (void, i32):
    (log(), +1)

fn erase() -> void:
    let value: Any = log()
    let unit: void = ()
`).filter((code) => code !== "unused-local-binding"),
    [],
  );
});

test("an if used as a value needs an else, and the unit pattern matches only void", () => {
  assert.deepEqual(
    codes(`fn pick(ready: bool) -> i32:
    value := if ready: +1
    +0
`),
    ["type-mismatch"],
  );
  assert.deepEqual(
    codes(`fn pick(value: i32) -> i32:
    match value:
        () => +1
`),
    ["type-mismatch"],
  );
});
