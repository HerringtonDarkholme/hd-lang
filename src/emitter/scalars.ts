import type { ValueType } from "../hir.ts";
import { numericType } from "../numeric.ts";

// Scalar values at run time (src/numeric.ts): every integer of at most 32
// bits, `bool`, and `char` is an `i32`; `i64` and `u64` are an `i64`. Boxed
// for erased generics, the `i32` and `i64` scalars use `$hd.box-i32` and
// `$hd.box-i64`, and both floats use `$hd.box-f64`: an `f32` widens exactly.

/** The Wasm value type of a scalar. */
export function scalarWasm(type: ValueType): "i32" | "i64" | "f32" | "f64" {
  return numericType(type)?.wasm ?? "i32";
}

function isScalar(type: ValueType): boolean {
  return numericType(type) !== undefined || type === "bool" || type === "char";
}

/**
 * A scalar boxed as an erased value; any other value unchanged. A `void`
 * slot holds null, whatever `value` is: a void expression that must run
 * boxes through `voidThen`.
 */
export function boxScalar(value: string, type: ValueType): string {
  if (type === "void") return `(ref.null any)`;
  if (!isScalar(type)) return value;
  const wasm = scalarWasm(type);
  if (wasm === "f32") return `(struct.new $hd.box-f64 (f64.promote_f32 ${value}))`;
  return `(struct.new $hd.box-${wasm} ${value})`;
}

/**
 * The void instruction `value`, run for its effects, then `then`. The unit
 * value `()` is `(nop)`, which needs no block.
 */
export function voidThen(value: string, then: string): string {
  return value === "(nop)" ? then : `(block (result anyref) ${value} ${then})`;
}

/** An erased value unboxed as a scalar, or undefined for another type. */
export function unboxScalar(payload: string, type: ValueType): string | undefined {
  if (!isScalar(type)) return undefined;
  const wasm = scalarWasm(type);
  const box = wasm === "f32" ? "f64" : wasm;
  const value = `(struct.get $hd.box-${box} $hd.box-${box}-value (ref.cast (ref $hd.box-${box}) ${payload}))`;
  return wasm === "f32" ? `(f32.demote_f64 ${value})` : value;
}
