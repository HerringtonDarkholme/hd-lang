import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

function codes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("ordinary and generic unknown requirement keys are unknown traits", () => {
  assert.deepEqual(codes("fn run() -> void $ Missing: pass"), ["unknown-trait"]);
  assert.deepEqual(
    codes(`data Item: pass
fn run() -> void $ Missing[Item]: pass`),
    ["unknown-trait"],
  );
});

test("requirement keys reject every source of dynamic unsafety", () => {
  assert.deepEqual(
    codes(`trait Factory:
    fn create() -> Self
fn run() -> void $ Factory: pass`),
    ["trait-not-dynamically-safe"],
  );
  assert.deepEqual(
    codes(`trait Factory:
    fn create() -> Self
trait Child < Factory
fn run() -> void $ Child: pass`),
    ["trait-not-dynamically-safe"],
  );
  assert.deepEqual(
    codes(`trait Transform:
    fn apply[T](self, value: T) -> T
fn run() -> void $ Transform: pass`),
    ["trait-not-dynamically-safe"],
  );
});

test("Inspectable subtraits are rejected at nested and provider key positions", () => {
  const declaration = `use std.inspect.Inspectable
trait Storage < Inspectable:
    fn get(self) -> string
`;
  assert.deepEqual(codes(`${declaration}fn take(callback: fn() -> void $ Storage) -> void: pass`), [
    "inspectable-requirement",
  ]);
  assert.deepEqual(
    codes(`${declaration}data Store: pass
impl Storage for Store:
    fn get(self) -> string: ""
fn run() -> void:
    $.with(Storage=Store {}):
        pass`),
    ["inspectable-requirement"],
  );
});

test("an unrelated user trait named Inspectable remains an ordinary key", () => {
  assert.deepEqual(
    codes(`trait Inspectable
fn run() -> void $ Inspectable: pass`),
    [],
  );
});

test("reference-only and row method generics preserve dynamic safety", () => {
  assert.deepEqual(
    codes(`trait Visitor:
    fn visit[T < AnyRef](self, value: T) -> void
fn run() -> void $ Visitor: pass`),
    [],
  );
  assert.deepEqual(
    codes(`trait Worker:
    fn run[$R](self, job: fn() -> void $ R) -> void $ R
fn run() -> void $ Worker: pass`),
    [],
  );
});

test("requirement keys bind all reachable associated types", () => {
  const declaration = `trait Store:
    type Item
    fn load(self) -> Self::Item
`;
  assert.deepEqual(codes(`${declaration}fn bad() -> void $ Store: pass`), [
    "trait-not-dynamically-safe",
  ]);
  assert.deepEqual(codes(`${declaration}fn good() -> void $ Store[Item = i32]: pass`), []);
  assert.deepEqual(
    codes(`${declaration}trait Child < Store
fn good() -> void $ Child[Item = i32]: pass`),
    [],
  );
});

test("requirement keys diagnose arity and associated binding structure", () => {
  assert.deepEqual(codes("trait Repo[T]\nfn run() -> void $ Repo: pass"), [
    "partial-generic-arguments",
  ]);
  const declaration = `trait Store:
    type Item
    fn load(self) -> Self::Item
`;
  assert.deepEqual(codes(`${declaration}fn run() -> void $ Store[Other = i32]: pass`), [
    "unknown-associated-type",
  ]);
  assert.deepEqual(codes(`${declaration}fn run() -> void $ Store[Item = i32, Item = i32]: pass`), [
    "duplicate-associated-binding",
  ]);
  assert.deepEqual(codes(`${declaration}fn run() -> void $ Store[Item = Missing]: pass`), [
    "unknown-type",
  ]);
});

test("rows nested inside associated binding values are validated", () => {
  assert.deepEqual(
    codes(`trait Store:
    type Item
    fn load(self) -> Self::Item
fn run() -> void $ Store[Item = fn() -> void $ Missing]: pass`),
    ["unknown-trait"],
  );
});

test("unknown key argument types are rejected", () => {
  assert.deepEqual(
    codes(`trait Repo[T]
fn run() -> void $ Repo[Missing]: pass`),
    ["unknown-type"],
  );
});

test("trait-value key arguments have one canonical provider identity", () => {
  assert.deepEqual(
    codes(`trait Marker
trait Repo[T]
data Provider: pass
impl Repo[Marker] for Provider

fn need() -> void $ Repo[Marker]: pass

fn run() -> void:
    $.with(Repo[Marker]=Provider {}):
        need()`),
    [],
  );
});

test("callable type rows are validated through nested nominal types", () => {
  assert.deepEqual(codes("fn take(callbacks: List[fn() -> void $ Missing]) -> void: pass"), [
    "unknown-trait",
  ]);
  assert.deepEqual(
    codes(`fn run() -> void:
    let callback: fn() -> void $ Missing = fn() -> void: pass`),
    ["unknown-trait"],
  );
});

test("data surfaces validate rows after every trait body is known", () => {
  assert.deepEqual(
    codes(`data Job:
    callback: fn() -> void $ Factory
trait Factory:
    fn create() -> Self`),
    ["trait-not-dynamically-safe"],
  );
});

test("row-kinded arguments validate their concrete keys", () => {
  assert.deepEqual(
    codes(`use std.function.Fn
fn take(job: Fn[(), void, $ Missing]) -> void: pass`),
    ["unknown-trait"],
  );
});

test("explicit closure rows use the same requirement-key validation", () => {
  assert.deepEqual(
    codes(`fn run() -> void:
    _ := fn() -> void $ Missing: pass`),
    ["unknown-trait"],
  );
});

test("provider bindings validate bare keys before their values", () => {
  assert.deepEqual(
    codes(`fn run() -> void:
    $.with(Missing=1):
        pass`),
    ["unknown-trait"],
  );
});

test("context types validate their key rows", () => {
  assert.deepEqual(codes("fn take(providers: $.Context[$ Missing]) -> void: pass"), [
    "unknown-trait",
  ]);
});

test("trait method rows are validated even without an implementation", () => {
  assert.deepEqual(
    codes(`trait Worker:
    fn run(self) -> void $ Missing`),
    ["unknown-trait"],
  );
});

test("marked row parameters remain symbolic rather than becoming trait keys", () => {
  assert.deepEqual(
    codes(`trait Worker:
    fn run[$R](self, job: fn() -> void $ R) -> void $ R`),
    [],
  );
});

test("aliases are expanded before requirement keys are validated", () => {
  assert.deepEqual(
    codes(`type MissingAlias = Missing
fn run() -> void $ MissingAlias: pass`),
    ["unknown-trait"],
  );
});
