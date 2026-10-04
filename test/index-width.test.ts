import assert from "node:assert/strict";
import test from "node:test";

import { compileToWasm, instantiate } from "../src/compiler.ts";

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
