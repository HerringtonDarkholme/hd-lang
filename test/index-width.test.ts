import assert from "node:assert/strict";
import test from "node:test";

import { compileToWasm, instantiate } from "../src/compiler.ts";
import { RuntimePanicError } from "../src/runtime-panic.ts";

function isIndexPanic(error: unknown): boolean {
  return error instanceof RuntimePanicError && error.code === "index-out-of-bounds";
}

test("u64 list reads and stores use checked wide runtime entry points", async () => {
  const source = `fn get(items: List[i32], at: u64) -> i32: items[at]
fn set(items: mut List[i32], at: u64, value: i32) -> void:
    items[at] = value

fn main() -> i32:
    let mut items = [7]
    set(items, u64(0), 9)
    get(items, u64(0))
`;
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /call \$hd\.vector_get_wide/);
  assert.match(compilation.wat, /call \$hd\.vector_set_wide/);
  assert.ok(WebAssembly.validate(compilation.bytes));

  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 9);
});

test("a u64 list index is bounds-checked before it can wrap to u32", async () => {
  const read = await instantiate(`fn main() -> i32: [7][u64(4294967296)]\n`);
  assert.throws(() => (read.instance.exports.main as CallableFunction)(), isIndexPanic);

  const store = await instantiate(`fn main() -> i32:
    let mut items = [7]
    items[u64(4294967296)] = 9
    items[0]
`);
  assert.throws(() => (store.instance.exports.main as CallableFunction)(), isIndexPanic);
});

test("a compound list index keeps its context and is evaluated once", async () => {
  const source = `fn next(counter: mut List[i32]) -> u64:
    counter[0] = counter[0] + 1
    u64(0)

fn main() -> i32:
    let mut counter = [0]
    let mut items = [1]
    items[next(counter)] += 2
    counter[0] * 10 + items[0]
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 13);
});
