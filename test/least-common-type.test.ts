import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

function diagnosticCodes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("optional injection uses one LCT across literals, if, match, and inferred results", async () => {
  const source = `fn unwrap(value: i32?) -> i32:
    match value:
        .Some(found) => found
        .None => 0

fn inferred(flag: bool, plain: i32, optional: i32?):
    if flag: plain else: optional

fn selected(key: i32, plain: i32, optional: i32?) -> i32?:
    match key:
        0 => plain
        _ => optional

fn main() -> i32:
    let none: i32? = .None
    from_if := if true: 10 else: none
    values := [20, none]
    from_match := selected(0, 5, none)
    from_result := inferred(false, 0, .Some(7))
    unwrap(from_if) + unwrap(values[0]) + unwrap(from_match) + unwrap(from_result)
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("declared and built-in variance contribute at every shared LCT site", () => {
  const source = `data User:
    value: i32

data Producer[+T]:
    value: T

data Consumer[-T]:
    consume: fn(T) -> void

data Envelope[+T]:
    value: T

fn inferred(a: Producer[mut User], b: Producer[User], flag: bool):
    if flag: a else: b

fn join_producers(producer_mut: Producer[mut User], producer_read: Producer[User]) -> List[Producer[User]]:
    producers := [producer_mut, producer_read]
    producers

fn join_maps(map_mut: Map[string, mut User], map_read: Map[string, User]) -> Map[string, Map[string, User]]:
    maps := {"mutable": map_mut, "readonly": map_read}
    maps

fn join_callbacks(callback_wide: fn(User) -> mut User, callback_exact: fn(mut User) -> User) -> List[fn(mut User) -> User]:
    callbacks := [callback_wide, callback_exact]
    callbacks

fn join_crossed_callbacks(left: fn(mut User, User) -> User, right: fn(User, mut User) -> User) -> List[fn(mut User, mut User) -> User]:
    callbacks := [left, right]
    callbacks

fn join_consumers(consumer_read: Consumer[User], consumer_mut: Consumer[mut User], flag: bool) -> Consumer[mut User]:
    match flag:
        true => consumer_read
        false => consumer_mut

fn check_if(map_mut: Map[string, mut User], map_read: Map[string, User], flag: bool) -> Map[string, User]:
    picked_map := if flag: map_mut else: map_read
    picked_map

fn nested(left: Envelope[Producer[mut User]], right: Envelope[Producer[User]], flag: bool) -> Envelope[Producer[User]]:
    if flag: left else: right
`;
  assert.deepEqual(diagnosticCodes(source), []);
});

test("LCT remains order-independent when variance arguments need a structural join", () => {
  const declarations = `data User: pass
data Pair[+A, +B]:
    left: A
    right: B
`;
  for (const arms of [
    ["both", "left", "right"],
    ["right", "both", "left"],
    ["left", "right", "both"],
  ]) {
    const source = `${declarations}
fn pick(
    key: i32,
    left: Pair[User, mut User],
    right: Pair[mut User, User],
    both: Pair[User, User]
) -> Pair[User, User]:
    value := match key:
        0 => ${arms[0]}
        1 => ${arms[1]}
        _ => ${arms[2]}
    value
`;
    assert.deepEqual(diagnosticCodes(source), []);
  }
});

test(
  "wide declarations solve variance positions without a candidate product",
  { timeout: 5_000 },
  () => {
    const count = 24;
    const parameters = Array.from({ length: count }, (_, index) => `+T${index}`).join(", ");
    const fields = Array.from({ length: count }, (_, index) => `    field${index}: T${index}`).join(
      "\n",
    );
    const left = Array.from({ length: count }, (_, index) =>
      index % 2 === 0 ? "mut User" : "User",
    ).join(", ");
    const right = Array.from({ length: count }, (_, index) =>
      index % 2 === 0 ? "User" : "mut User",
    ).join(", ");
    const source = `data User: pass
data Wide[${parameters}]:
${fields}
fn join(left: Wide[${left}], right: Wide[${right}]) -> void:
    _ := [left, right]
`;
    assert.deepEqual(diagnosticCodes(source), []);
  },
);

test("LCT rejects two optional layers and distinguishes a forbidden combined step", () => {
  assert.deepEqual(
    diagnosticCodes(`fn invalid(flag: bool, value: i32, nested: i32??) -> void:
    _ := if flag: value else: nested
`),
    ["no-common-type"],
  );
  assert.deepEqual(
    diagnosticCodes(`data User: pass
fn invalid(small: mut List[mut User], wide: List[User]) -> void:
    _ := [small, wide]
`),
    ["no-least-common-type"],
  );
});
