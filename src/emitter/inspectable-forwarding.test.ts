import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";

async function runMain(source: string): Promise<number> {
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  return (instance.exports.main as CallableFunction)();
}

const namedTypes = `use std.inspect.{Inspectable, TypeId}

trait Named < Inspectable & AnyRef:
    fn name(self) -> string

data User:
    name: string

impl Named for User:
    fn name(self) -> string: self.name
`;

test("forwarded inspection distinguishes dynamic and static type identity", async () => {
  const source = `${namedTypes}
fn identities_differ[T < AnyRef & Inspectable](value: T) -> bool:
    value.runtime_type() != TypeId::of::[T]()

fn name_of[T < Named](value: T) -> string:
    value.name()

fn main() -> i32:
    let value: Named = User { name: "Ada" }
    if identities_differ(value) && name_of(value) == "Ada": 42 else: 0
`;
  assert.equal(await runMain(source), 42);
});

test("a dynamic trait is an exact generic downcast target", async () => {
  const source = `${namedTypes}
fn recover[T < AnyRef & Inspectable](value: Inspectable) -> T?:
    value.downcast::[T]()

fn main() -> i32:
    let value: Named = User { name: "Ada" }
    match recover::[Named](value):
        .Some(_) => 0
        .None => 42
`;
  assert.equal(await runMain(source), 42);
});

test("concrete generic downcasts still recover the same reference", async () => {
  const source = `${namedTypes}
fn recover[T < AnyRef & Inspectable](value: Inspectable) -> T?:
    value.downcast::[T]()

fn main() -> i32:
    user := User { name: "Ada" }
    let value: Inspectable = user
    match recover::[User](value):
        .Some(found) =>
            if found is user: 42 else: 0
        .None => 0
`;
  assert.equal(await runMain(source), 42);
});

test("an Error bound supplies transitive Inspectable evidence", async () => {
  const source = `use std.error.Error

data Failure: pass

impl Display for Failure:
    fn to_string(self) -> string: "failure"

impl Error for Failure

fn recover[T < Error](value: Error) -> T?:
    value.downcast::[T]()

fn main() -> i32:
    failure := Failure {}
    let value: Error = failure
    match recover::[Failure](value):
        .Some(found) =>
            if found is failure: 42 else: 0
        .None => 0
`;
  assert.equal(await runMain(source), 42);
});

test("nested static identity retains a mutable dynamic trait argument", async () => {
  const source = `${namedTypes}
fn nested_name[T < AnyRef & Inspectable]() -> string:
    TypeId::of::[List[T]]().to_string()

fn main() -> i32:
    if nested_name::[mut Named]() == "List[mut Named]": 42 else: 0
`;
  assert.equal(await runMain(source), 42);
});
