import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";

const pendingOnce = (_functionIndex: number, pollCount: number): boolean => pollCount === 1;

test("a nested return after a direct drive completes the frame and unwinds defer once", async () => {
  const source = `data State:
    count: i32

fn child!() -> i32: 40

fn run!(state: mut State) -> i32:
    defer:
        state.count = state.count + 1
    value := child!()
    if value == 40:
        return value + 2
    0

fn main!() -> i32:
    let mut state = State { count: 0 }
    result := run!(state)
    result + state.count
`;
  for (const pending of [undefined, pendingOnce]) {
    const { instance } = await instantiate(source, { pending });
    assert.equal((instance.exports.main as CallableFunction)(), 43);
  }
});

test("nested returns after a direct drive use the language result type, not the poll type", async () => {
  const source = `fn child!() -> i32: 1

fn wide!() -> i64:
    value := child!()
    if value == 1:
        return 42
    0

fn main!() -> i32:
    if wide!() == 42: 42 else: 0
`;
  const { instance } = await instantiate(source, { pending: pendingOnce });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("a nested void return after a direct drive completes with poll-ready", async () => {
  const source = `data State:
    count: i32

fn child!() -> i32: 1

fn run!(state: mut State):
    defer:
        state.count = state.count + 1
    value := child!()
    if value == 1:
        return
    state.count = 100

fn main!() -> i32:
    let mut state = State { count: 0 }
    run!(state)
    state.count
`;
  const { instance } = await instantiate(source, { pending: pendingOnce });
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});

test("a void return operand runs once before suspension cleanup", async () => {
  const source = `data State:
    count: i32

fn child!() -> i32: 1

fn tick(state: mut State):
    state.count = state.count + 1

fn run!(state: mut State):
    defer:
        state.count = state.count * 10
    _ := child!()
    return tick(state)

fn main!() -> i32:
    let mut state = State { count: 0 }
    run!(state)
    state.count
`;
  const { instance } = await instantiate(source, { pending: pendingOnce });
  assert.equal((instance.exports.main as CallableFunction)(), 10);
});

test("separate Result propagation after a direct drive completes and unwinds the frame", async () => {
  const source = `data State:
    count: i32

data Failure:
    code: i32

fn child!(ok: bool) -> Result[i32, Failure]:
    if ok: .Ok(40) else: .Err(Failure { code: 7 })

fn run!(state: mut State, ok: bool) -> Result[i32, Failure]:
    defer:
        state.count = state.count + 1
    answer := child!(ok)
    value := answer?
    .Ok(value + 2)

fn inspect(value: Result[i32, Failure]) -> i32:
    match value:
        .Ok(actual) => actual
        .Err(error) => -error.code

fn main!() -> i32:
    let mut state = State { count: 0 }
    success := inspect(run!(state, true))
    failure := inspect(run!(state, false))
    success + failure + state.count
`;
  for (const pending of [undefined, pendingOnce]) {
    const { instance } = await instantiate(source, { pending });
    assert.equal((instance.exports.main as CallableFunction)(), 37);
  }
});

test("non-suspending comprehensions propagate through the surrounding suspension frame", async () => {
  const cases = [
    ["List[i32]", "[for item in [ok] => parse(item)?]"],
    ["Map[i32, i32]", "{for item in [ok] => 0: parse(item)?}"],
    ["Map[i32, i32]", "{for item in [ok] => parse(item)?: 1}"],
  ];
  for (const [collection, comprehension] of cases) {
    const source = `data State:
    count: i32

data Failure:
    code: i32

fn child!() -> i32: 40

fn parse(ok: bool) -> Result[i32, Failure]:
    if ok: .Ok(40) else: .Err(Failure { code: 7 })

fn run!(state: mut State, ok: bool) -> Result[${collection}, Failure]:
    defer:
        state.count = state.count + 1
    _ := child!()
    .Ok(${comprehension})

fn inspect(value: Result[${collection}, Failure]) -> i32:
    match value:
        .Ok(actual) => actual.len() + 39
        .Err(error) => -error.code

fn main!() -> i32:
    let mut state = State { count: 0 }
    success := inspect(run!(state, true))
    failure := inspect(run!(state, false))
    success + failure + state.count
`;
    const { instance } = await instantiate(source, { pending: pendingOnce });
    assert.equal((instance.exports.main as CallableFunction)(), 35);
  }
});

test("cleanup cannot propagate out of an ordinary or suspending function", () => {
  for (const name of ["run", "run!"]) {
    const source = `fn ${name}() -> Result[i32, string]:
    defer:
        let answer: Result[i32, string] = .Err("failure")
        _ := answer?
    .Ok(42)
`;
    assert.equal(analyze(source).diagnostics[0]?.code, "defer-control-flow");
  }
});

test("a closure declared in cleanup can propagate within its own function", async () => {
  const source = `fn main() -> i32:
    defer:
        inner := fn() -> Result[i32, string]:
            let answer: Result[i32, string] = .Err("failure")
            _ := answer?
            .Ok(0)
        _ := inner()
    42
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
