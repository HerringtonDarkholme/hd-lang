import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

function codes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("Error rejects an AnyVal newtype target", () => {
  assert.deepEqual(
    codes(`use std.error.Error

type Message(string)

impl Display for Message:
    fn to_string(self) -> string: string(self)

impl Error for Message`),
    ["missing-supertrait-implementation"],
  );
});

test("category supertraits are enforced without knowing the trait by name", () => {
  assert.deepEqual(
    codes(`trait RefOnly < AnyRef
type Message(string)
impl RefOnly for Message`),
    ["missing-supertrait-implementation"],
  );
  assert.deepEqual(
    codes(`trait RefOnly < AnyRef
data User: pass
type Owner(User)
impl RefOnly for Owner`),
    [],
  );
});

test("transitively implied AnyRef bounds preserve dynamic safety", () => {
  assert.deepEqual(
    codes(`trait Registry:
    fn lookup[T < Child](self, name: string) -> T?

trait Child < Base
trait Base < AnyRef

fn valid(registry: Registry) -> void:
    pass`),
    [],
  );
  assert.deepEqual(
    codes(`use std.error.Error

trait Registry:
    fn lookup[T < Error](self, name: string) -> T?

fn valid(registry: Registry) -> void:
    pass`),
    [],
  );
});

test("an implied AnyVal method parameter remains dynamically unsafe", () => {
  assert.deepEqual(
    codes(`trait Values < AnyVal
trait Registry:
    fn lookup[T < Values](self, name: string) -> T?
fn invalid(registry: Registry) -> void: pass`),
    ["trait-not-dynamically-safe"],
  );
});

test("generic newtypes prove category supertraits through their impl bounds", () => {
  const declarations = `trait RefOnly < AnyRef
type Wrap[T](T)
`;
  assert.deepEqual(codes(`${declarations}impl[T < AnyRef] RefOnly for Wrap[T]`), []);
  assert.deepEqual(codes(`${declarations}impl[T] RefOnly for Wrap[T]`), [
    "missing-supertrait-implementation",
  ]);
});

test("top-level generic signatures retain implied reference categories", () => {
  assert.deepEqual(
    codes(`trait RefOnly < AnyRef
fn same[T < RefOnly](left: T, right: T) -> bool:
    left is right`),
    [],
  );
});
