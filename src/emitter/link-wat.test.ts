import assert from "node:assert/strict";
import test from "node:test";

import { assembleWat } from "../wasm.ts";
import { linkWat } from "./link-wat.ts";

test("exports retain transitive function and global dependencies, not declaration elements", async () => {
  const source = `(module
    (global $answer i32 (i32.const 42))
    (func $helper (result i32) (global.get $answer))
    (func $dead (result i32) (i32.const 0))
    (func $main (export "main") (result i32) (call $helper))
    (elem declare func $helper $dead)
  )`;
  const result = linkWat(source);
  assert.ok(result.includes("$helper"));
  assert.ok(result.includes("$answer"));
  assert.ok(!result.includes("$dead"));
  assert.equal(linkWat(result), result);
  const { bytes } = await assembleWat(result);
  const { instance } = await WebAssembly.instantiate(bytes);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("unused recursive type components and imports disappear", async () => {
  const source = `(module
    (import "host" "unused" (func $unused))
    (rec
      (type $live (struct (field (ref null $live))))
      (type $dead (struct (field (ref null $dead)))))
    (func $main (export "main") (result (ref null $live)) (ref.null $live))
  )`;
  const result = linkWat(source);
  assert.ok(result.includes("$live"));
  assert.ok(!result.includes("$dead"));
  assert.ok(!result.includes("$unused"));
  await assembleWat(result);
});

test("quoted export names and nested comments are not declaration references", async () => {
  const source = `(module
    (func $dead)
    (; ignored ($dead (; nested ;) ) ;)
    (func $main (export "$dead \\" quoted")
      ;; (call $dead)
      (nop))
  )`;
  const result = linkWat(source);
  assert.ok(!result.includes("(func $dead"));
  assert.ok(result.includes('(export "$dead \\" quoted")'));
  await assembleWat(result);
});

test("start is an execution root and ref.func keeps a callable declaration", async () => {
  const source = `(module
    (type $callback (func))
    (global $slot (mut (ref null $callback)) (ref.null $callback))
    (func $body (type $callback))
    (func $start (global.set $slot (ref.func $body)))
    (start $start)
    (elem declare func $body)
  )`;
  const result = linkWat(source);
  assert.ok(result.includes("(func $body"));
  assert.ok(result.includes("(start $start)"));
  await assembleWat(result);
});
