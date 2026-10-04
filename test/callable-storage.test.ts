import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

async function runMain(
  source: string,
  pending?: (_functionIndex: number, pollCount: number) => boolean,
): Promise<number> {
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source, { pending });
  return (instance.exports.main as CallableFunction)();
}

test("generic lists invoke stored callables", async () => {
  const source = `fn call_first[T](tasks: List[fn() -> T]) -> T:
    task := tasks[0]
    task()

fn main() -> i32:
    call_first([fn() -> i32: 42])
`;
  assert.equal(await runMain(source), 42);
});

test("generic callable-list results retain their concrete callable ABI", async () => {
  const source = `fn forty_two() -> i32: 42

fn singleton[T](task: fn() -> T) -> List[fn() -> T]:
    [task]

fn main() -> i32:
    tasks := singleton(forty_two)
    task := tasks[0]
    task()
`;
  assert.equal(await runMain(source), 42);
});

test("generic lists invoke callables with erased inputs", async () => {
  const source = `fn apply_first[T](tasks: List[fn(T) -> i32], value: T) -> i32:
    task := tasks[0]
    task(value)

fn main() -> i32:
    apply_first([fn(value: i32) -> i32: value + 1], 41)
`;
  assert.equal(await runMain(source), 42);
});

test("nested generic storage uses the same callable representation", async () => {
  const source = `data Box[T]:
    value: T

fn invoke[T](box: Box[List[fn() -> T]]) -> T:
    task := box.value[0]
    task()

fn main() -> i32:
    invoke(Box { value: [fn() -> i32: 42] })
`;
  assert.equal(await runMain(source), 42);
});

test("generic tuples invoke stored callables", async () => {
  const source = `fn invoke[T](pair: (fn() -> T, i32)) -> T:
    let (task, _) = pair
    task()

fn main() -> i32:
    invoke((fn() -> i32: 42, 0))
`;
  assert.equal(await runMain(source), 42);
});

test("generic maps invoke stored callables", async () => {
  const source = `fn invoke[T](tasks: Map[string, fn() -> T], fallback: T) -> T:
    match tasks.get("answer"):
        .Some(task) => task()
        .None => fallback

fn main() -> i32:
    invoke({"answer": fn() -> i32: 42}, 0)
`;
  assert.equal(await runMain(source), 42);
});

test("callable storage preserves mutable list identity", async () => {
  const source = `fn forty() -> i32: 40
fn two() -> i32: 2

fn mutate[T](tasks: mut List[fn() -> T], task: fn() -> T) -> mut List[fn() -> T]:
    tasks.push(task)
    tasks

fn main() -> i32:
    let tasks: mut List[fn() -> i32] = [forty]
    returned := mutate(tasks, two)
    first := returned[0]
    second := tasks[1]
    if returned is tasks: first() + second() else: 0
`;
  assert.equal(await runMain(source), 42);
});

test("generic provider rows survive callable storage", async () => {
  const source = `trait Answer:
    fn get(self) -> i32

data Fixed: pass

impl Answer for Fixed:
    fn get(self) -> i32: 42

fn read() -> i32 $ Answer:
    $.use(Answer).get()

fn invoke_first[T, $R](tasks: List[fn() -> T $ R]) -> T $ R:
    task := tasks[0]
    task()

fn main() -> i32:
    $.with(Answer=Fixed {}):
        invoke_first([read])
`;
  assert.equal(await runMain(source), 42);
});

test("generic lists invoke suspending callables after pending", async () => {
  const source = `fn invoke_first![T](tasks: List[fn!() -> T]) -> T:
    task := tasks[0]
    task!()

fn main!() -> i32:
    invoke_first!([fn!() -> i32: 42])
`;
  assert.equal(await runMain(source), 42);
  assert.equal(await runMain(source, (_functionIndex, pollCount) => pollCount === 1), 42);
});
