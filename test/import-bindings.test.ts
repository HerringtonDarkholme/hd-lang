import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import { ImportBindingMap } from "../src/checker/import-bindings.ts";

test("import bindings share one stored declaration", () => {
  const declaration = { name: "second" };
  const bindings = new ImportBindingMap(new Map([["first", "second"]]));
  bindings.set("second", declaration);

  assert.equal(bindings.get("first"), declaration);
  assert.equal(bindings.get("second"), declaration);
  assert.deepEqual([...bindings.keys()], ["second"]);

  const shadow = { name: "first" };
  bindings.set("first", shadow);
  assert.equal(bindings.get("first"), shadow);
  assert.equal(bindings.get("second"), declaration);
});

test("two aliases of one std function check and emit one declaration", async () => {
  const source = `use std.cmp.min as first
use std.cmp.min as second

fn call(first: fn(i32, i32) -> i32) -> i32:
    first(second(3, 2), 1)

fn main() -> i32:
    first(second(3, 2), 1) + call(fn(a: i32, b: i32) -> i32: a + b)
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 4);
});

test("std type aliases share canonical generic, trait, enum, and newtype identities", () => {
  const source = `use std.ops.Range as FirstRange
use std.ops.Range as SecondRange
use std.ops.Add as FirstAdd
use std.ops.Add as SecondAdd
use std.cmp.Ordering as FirstOrder
use std.cmp.Ordering as SecondOrder
use std.hash.DefaultHasher as FirstHasher
use std.hash.DefaultHasher as SecondHasher
use std.process.ExitCode as FirstCode
use std.process.ExitCode as SecondCode
use std.ops.Default

fn first_range() -> FirstRange[i32]:
    FirstRange::[i32] { start: 1, end: 2, inclusive: false }

fn second_range() -> SecondRange[i32]:
    SecondRange::[i32] { start: 2, end: 3, inclusive: true }

fn first_order() -> FirstOrder:
    FirstOrder.Less

fn second_order() -> SecondOrder:
    SecondOrder.Greater

fn first_hasher() -> FirstHasher:
    FirstHasher::new()

fn second_hasher_factory() -> fn() -> mut SecondHasher:
    SecondHasher::new

fn first_code() -> FirstCode:
    FirstCode(1)

fn second_code() -> SecondCode:
    SecondCode(2)

fn first_add[T < FirstAdd](value: T) -> T:
    value

fn second_add[T < SecondAdd](value: T) -> T:
    value

fn generic_shadows_alias[FirstCode < Default]() -> FirstCode:
    FirstCode::default()
`;
  assert.deepEqual(analyze(source).diagnostics, []);
});
