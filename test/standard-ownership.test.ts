import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function codes(source: string): string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("a user cannot implement a standard trait for a standard data type", () => {
  assert.deepEqual(
    codes(`use std.time.Duration

impl Hash for Duration:
    fn hash(self, hasher: mut Hasher) -> void:
        self.as_milliseconds().hash(hasher)
`),
    ["orphan-impl"],
  );
});

test("a user cannot implement a standard trait for a standard enum", () => {
  assert.deepEqual(
    codes(`use std.console.ConsoleError

impl Hash for ConsoleError:
    fn hash(self, hasher: mut Hasher) -> void:
        0.hash(hasher)
`),
    ["orphan-impl"],
  );
});

test("a user owns its data type when implementing a standard trait", () => {
  assert.deepEqual(
    codes(`data Local:
    value: i32

impl Hash for Local:
    fn hash(self, hasher: mut Hasher) -> void:
        self.value.hash(hasher)
`),
    [],
  );
});

test("a user-owned trait may be implemented for a standard type", () => {
  assert.deepEqual(
    codes(`use std.time.Duration

trait Marker

impl Marker for Duration
`),
    [],
  );
});
