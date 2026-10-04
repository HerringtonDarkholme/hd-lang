import assert from "node:assert/strict";
import test from "node:test";

import { emitIntrinsicBody } from "../src/emitter/intrinsics.ts";
import type { HirFunction } from "../src/hir.ts";
import { assembleWat } from "../src/wasm.ts";

// The `char_from_scalar` primitive (spec/std/README.md#standard-library-primitives)
// never sees a bad value from `lib/std`, so this assembles its body alone.
test("char_from_scalar gives .None for a surrogate or a value above 0x10FFFF", async () => {
  const declaration = {
    intrinsic: "char_from_scalar",
    parameters: [{ index: 0 }],
    result: "i32",
  } as unknown as HirFunction;
  const body = emitIntrinsicBody(declaration, (value) => value);
  const wat = `(module
    (type $hd.box-i32 (struct (field $hd.box-i32-value i32)))
    (type $hd.variant (struct (field $hd.variant-tag i32) (field $hd.variant-payload (mut anyref))))
    (func $from (param $l0 i32) (result (ref $hd.variant))
      ${body})
    (func (export "tag") (param i32) (result i32)
      (struct.get $hd.variant $hd.variant-tag (call $from (local.get 0))))
    (func (export "value") (param i32) (result i32)
      (struct.get $hd.box-i32 $hd.box-i32-value
        (ref.cast (ref $hd.box-i32)
          (struct.get $hd.variant $hd.variant-payload (call $from (local.get 0)))))))`;
  const { bytes } = await assembleWat(wat);
  const { instance } = await WebAssembly.instantiate(bytes);
  const tag = instance.exports.tag as (point: number) => number;
  const value = instance.exports.value as (point: number) => number;
  for (const point of [0, 0x41, 0xd7ff, 0xe000, 0x10ffff]) {
    assert.equal(tag(point), 1, `0x${point.toString(16)} is a char`);
    assert.equal(value(point), point);
  }
  for (const point of [0xd800, 0xdbff, 0xdc00, 0xdfff, 0x110000, -1])
    assert.equal(tag(point), 0, `0x${(point >>> 0).toString(16)} has no char`);
});
