import type { ValueType } from "./hir.ts";

// The sized numeric types (spec/lang/04-type-system.md#primitive-types) and how
// the prototype represents them: every integer of at most 32 bits is a Wasm
// `i32`, `i64` and `u64` are a Wasm `i64`, and `f32` and `f64` are native.

export interface NumericType {
  readonly family: "signed" | "unsigned" | "float";
  readonly bits: 8 | 16 | 32 | 64;
  /** The Wasm value type at run time. */
  readonly wasm: "i32" | "i64" | "f32" | "f64";
  /** An integer type's range. */
  readonly minimum?: bigint;
  readonly maximum?: bigint;
}

const integer = (family: "signed" | "unsigned", bits: 8 | 16 | 32 | 64): NumericType => ({
  family,
  bits,
  wasm: bits === 64 ? "i64" : "i32",
  minimum: family === "signed" ? -(2n ** BigInt(bits - 1)) : 0n,
  maximum: family === "signed" ? 2n ** BigInt(bits - 1) - 1n : 2n ** BigInt(bits) - 1n,
});

export const NUMERIC_TYPES: ReadonlyMap<ValueType, NumericType> = new Map([
  ["i8", integer("signed", 8)],
  ["i16", integer("signed", 16)],
  ["i32", integer("signed", 32)],
  ["i64", integer("signed", 64)],
  ["u8", integer("unsigned", 8)],
  ["u16", integer("unsigned", 16)],
  ["u32", integer("unsigned", 32)],
  ["u64", integer("unsigned", 64)],
  ["f32", { family: "float", bits: 32, wasm: "f32" }],
  ["f64", { family: "float", bits: 64, wasm: "f64" }],
]);

export function numericType(type: ValueType | undefined): NumericType | undefined {
  return type === undefined ? undefined : NUMERIC_TYPES.get(type);
}

export function isIntegerType(type: ValueType | undefined): boolean {
  const numeric = numericType(type);
  return numeric !== undefined && numeric.family !== "float";
}

/** The integer types whose run-time value is a Wasm `i32`. */
export function isNarrowInteger(type: ValueType | undefined): boolean {
  return isIntegerType(type) && numericType(type)!.wasm === "i32";
}

/**
 * Whether `to` is a wider type of `from`'s family (spec/lang/04-type-system.md#r-types.num.families):
 * within one integer family to at least as many bits, or `f32` to `f64`.
 */
export function widensTo(from: ValueType, to: ValueType): boolean {
  const source = numericType(from);
  const target = numericType(to);
  if (!source || !target || from === to) return false;
  return source.family === target.family && source.bits < target.bits;
}

/** Whether `from` narrows implicitly to `to` within one family, which is an error. */
export function narrowsTo(from: ValueType, to: ValueType): boolean {
  return widensTo(to, from);
}

/** The wider of two numeric types of one family, or undefined. */
export function widerNumeric(left: ValueType, right: ValueType): ValueType | undefined {
  if (left === right) return left;
  if (widensTo(left, right)) return right;
  if (widensTo(right, left)) return left;
  return undefined;
}

/** The next wider type of an integer's family, suggested by a range diagnostic. */
export function widerIntegerName(type: ValueType): ValueType | undefined {
  const numeric = numericType(type);
  if (!numeric || numeric.family === "float" || numeric.bits === 64) return undefined;
  return `${numeric.family === "signed" ? "i" : "u"}${numeric.bits * 2}`;
}
