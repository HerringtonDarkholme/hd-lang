import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function codes(source: string): string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("a type parameter after a callable row is not mistaken for a row parameter", () => {
  assert.deepEqual(
    codes(`trait Show:
    fn show[T](self, pair: (fn() -> void $ Console, T)) -> i32

fn use_it(value: Show) -> i32: 0
`),
    ["trait-not-dynamically-safe"],
  );
});

test("dynamic safety is independent of tuple element order", () => {
  assert.deepEqual(
    codes(`trait Show:
    fn show[T](self, pair: (T, fn() -> void $ Console)) -> i32

fn use_it(value: Show) -> i32: 0
`),
    ["trait-not-dynamically-safe"],
  );
});

test("a declared method row parameter remains dynamically safe", () => {
  assert.deepEqual(
    codes(`trait Runner:
    fn run[$R](self, job: fn() -> void $ R) -> void $ R

fn keep(value: Runner) -> i32: 0
`),
    [],
  );
});
