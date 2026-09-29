import type { HirExpression, ValueType } from "../hir.ts";
import { numericType, type NumericType } from "../numeric.ts";
import type { RuntimePanicName } from "../runtime-panic.ts";

// Operations on the sized numeric types beyond `i32`, `i64`, and `f64`
// (spec/04-type-system.md#numeric-conversions), as WAT. An integer of at most
// 16 bits computes in `i32` and range-checks the result; `u32` computes in
// `i64` or with unsigned instructions; `u64` uses unsigned instructions and
// checked runtime helpers; `f32` is native.

export interface SizedNumericContext {
  allocateTemporary(type: ValueType): string;
  emitRuntimePanic(name: RuntimePanicName): string;
  emitCheckedDivision(
    width: "i32" | "i64",
    operator: "/" | "%",
    left: string,
    right: string,
  ): string;
  /** Records that the module imports `pow_f64`. */
  useFloatPower(): void;
  /** Records that the module imports `rem_f64`, the truncated floating remainder. */
  useFloatRemainder(): void;
}

/** The types these helpers handle; `i32`, `i64`, and `f64` keep their own paths. */
export function isSizedNumeric(type: ValueType): boolean {
  return ["i8", "i16", "u8", "u16", "u32", "u64", "f32"].includes(type);
}

function info(type: ValueType): NumericType {
  const numeric = numericType(type);
  if (!numeric) throw new Error(`'${type}' is not numeric`);
  return numeric;
}

function i32Constant(value: bigint): string {
  return `(i32.const ${BigInt.asIntN(32, value)})`;
}

function i64Constant(value: bigint): string {
  return `(i64.const ${BigInt.asIntN(64, value)})`;
}

/** An integer literal of any integer type. */
export function integerConstant(type: ValueType, value: number, wide: string | undefined): string {
  const exact = BigInt(wide ?? value);
  return info(type).wasm === "i64" ? i64Constant(exact) : i32Constant(exact);
}

/** A value of an `i32`-represented narrow type checked against its range. */
function checkNarrow(value: string, type: ValueType): string {
  const { minimum, maximum } = info(type);
  return `(call $hd.check_range_i32 ${value} ${i32Constant(minimum!)} ${i32Constant(maximum!)})`;
}

/** An integer value as an `i64` holding its mathematical value (`u64` stays bit-exact). */
function asWide(value: string, type: ValueType): string {
  const numeric = info(type);
  if (numeric.wasm === "i64") return value;
  return `(i64.extend_i32_${numeric.family === "unsigned" ? "u" : "s"} ${value})`;
}

/** Implicit widening within a family, or `f32` to `f64` (always exact). */
export function emitWiden(value: string, from: ValueType, to: ValueType): string {
  const source = info(from);
  const target = info(to);
  if (source.family === "float") return `(f64.promote_f32 ${value})`;
  if (source.wasm === target.wasm) return value;
  return asWide(value, from);
}

/** A constructor-style numeric cast (spec/04-type-system.md#numeric-casts). */
export function emitCast(value: string, from: ValueType, to: ValueType): string {
  if (from === to) return value;
  const source = info(from);
  const target = info(to);
  if (source.family === "float" && target.family === "float")
    return target.bits === 64 ? `(f64.promote_f32 ${value})` : `(f32.demote_f64 ${value})`;
  if (target.family === "float")
    return `(${target.wasm}.convert_${source.wasm}_${source.family === "unsigned" ? "u" : "s"} ${value})`;
  if (source.family === "float") {
    // A float-to-integer cast truncates toward zero and saturates: out of
    // range clamps to the target's bounds and NaN gives 0 (types.cast.saturate),
    // as the saturating `trunc_sat` instructions do. A narrow target clamps
    // first, in `f64`, where every narrow bound is exact.
    const widened = source.bits === 32 ? `(f64.promote_f32 ${value})` : value;
    const sign = target.family === "unsigned" ? "u" : "s";
    if (target.bits >= 32) return `(${target.wasm}.trunc_sat_f64_${sign} ${widened})`;
    const clamped = `(f64.min (f64.max ${widened} (f64.const ${target.minimum!})) (f64.const ${target.maximum!}))`;
    return `(i32.trunc_sat_f64_s ${clamped})`;
  }
  // An integer-to-integer cast wraps (types.cast.wrap): keep the low bits of
  // the two's-complement value and read them in the target type.
  const wide = asWide(value, from);
  if (target.wasm === "i64") return wide;
  const low = `(i32.wrap_i64 ${wide})`;
  if (target.bits === 32) return low;
  if (target.family === "unsigned") return `(i32.and ${low} ${i32Constant(target.maximum!)})`;
  const shift = `(i32.const ${32 - target.bits})`;
  return `(i32.shr_s (i32.shl ${low} ${shift}) ${shift})`;
}

/** Unary `-` (signed and float types) and `~`. */
export function emitSizedUnary(operator: string, value: string, type: ValueType): string {
  const numeric = info(type);
  if (operator === "~") {
    if (numeric.family === "unsigned" && numeric.wasm === "i32" && numeric.bits < 32)
      return `(i32.xor ${value} ${i32Constant(numeric.maximum!)})`;
    return `(${numeric.wasm}.xor ${value} (${numeric.wasm}.const -1))`;
  }
  if (numeric.family === "float") return `(${numeric.wasm}.neg ${value})`;
  return checkNarrow(`(i32.sub (i32.const 0) ${value})`, type);
}

/** A binary operator whose left operand has a sized numeric type. */
export function emitSizedBinary(
  operator: string,
  left: string,
  right: string,
  type: ValueType,
  rightType: ValueType,
  context: SizedNumericContext,
): string {
  const numeric = info(type);
  const wasm = numeric.wasm;
  const unsigned = numeric.family === "unsigned";
  if (numeric.family === "float") {
    if (operator === "**") {
      context.useFloatPower();
      return `(f32.demote_f64 (call $hd.pow_f64 (f64.promote_f32 ${left}) (f64.promote_f32 ${right})))`;
    }
    // The f64 remainder of two f32 values is exact, so demoting it is too.
    if (operator === "%") {
      context.useFloatRemainder();
      return `(f32.demote_f64 (call $hd.rem_f64 (f64.promote_f32 ${left}) (f64.promote_f32 ${right})))`;
    }
    const float: Readonly<Record<string, string>> = {
      "+": "add",
      "-": "sub",
      "*": "mul",
      "/": "div",
      "<": "lt",
      "<=": "le",
      ">": "gt",
      ">=": "ge",
    };
    return `(f32.${float[operator]} ${left} ${right})`;
  }
  const comparisons: Readonly<Record<string, string>> = {
    "<": "lt",
    "<=": "le",
    ">": "gt",
    ">=": "ge",
  };
  if (comparisons[operator])
    return `(${wasm}.${comparisons[operator]}_${unsigned ? "u" : "s"} ${left} ${right})`;
  if (["&", "|", "^"].includes(operator)) {
    const bitwise: Readonly<Record<string, string>> = { "&": "and", "|": "or", "^": "xor" };
    return `(${wasm}.${bitwise[operator]} ${left} ${right})`;
  }
  if (operator === "**") {
    const exponent = numericType(rightType)?.wasm === "i64" ? `(i32.wrap_i64 ${right})` : right;
    if (type === "u64") return `(call $hd.pow_u64 ${left} ${exponent})`;
    const power = `(call $hd.pow_i64 ${asWide(left, type)} ${exponent})`;
    if (type === "u32") return `(call $hd.check_u32 ${power})`;
    const { minimum, maximum } = numeric;
    return `(i32.wrap_i64 (call $hd.check_range_i64 ${power} ${i64Constant(minimum!)} ${i64Constant(maximum!)}))`;
  }
  if (operator === "<<" || operator === ">>") {
    const count =
      wasm === "i64"
        ? `(call $hd.check_shift_i64 ${right})`
        : `(call $hd.check_shift ${right} (i32.const ${numeric.bits}))`;
    if (operator === ">>") return `(${wasm}.shr_${unsigned ? "u" : "s"} ${left} ${count})`;
    const shifted = `(${wasm}.shl ${left} ${count})`;
    // A narrow left shift keeps the value's width, as `i32` wraps.
    if (numeric.bits === 8 || numeric.bits === 16)
      return unsigned
        ? `(i32.and ${shifted} ${i32Constant(numeric.maximum!)})`
        : `(i32.shr_s (i32.shl ${shifted} (i32.const ${32 - numeric.bits})) (i32.const ${32 - numeric.bits}))`;
    return shifted;
  }
  if (operator === "/" || operator === "%") {
    if (!unsigned)
      return checkNarrow(context.emitCheckedDivision("i32", operator, left, right), type);
    const leftTemporary = context.allocateTemporary(type);
    const rightTemporary = context.allocateTemporary(type);
    return [
      `(block (result ${wasm})`,
      `  (local.set ${leftTemporary} ${left})`,
      `  (local.set ${rightTemporary} ${right})`,
      `  (if (${wasm}.eqz (local.get ${rightTemporary}))`,
      `    (then ${context.emitRuntimePanic("integer-division-by-zero")}))`,
      `  (${wasm}.${operator === "/" ? "div_u" : "rem_u"} (local.get ${leftTemporary}) (local.get ${rightTemporary})))`,
    ].join("\n");
  }
  const arithmetic: Readonly<Record<string, string>> = { "+": "add", "-": "sub", "*": "mul" };
  const name = arithmetic[operator];
  if (!name) throw new Error(`unsupported operator '${operator}' on ${type}`);
  if (type === "u64") return `(call $hd.${name}_u64 ${left} ${right})`;
  if (type === "u32")
    return `(call $hd.check_u32 (i64.${name} (i64.extend_i32_u ${left}) (i64.extend_i32_u ${right})))`;
  return checkNarrow(`(i32.${name} ${left} ${right})`, type);
}

/**
 * A shift count of another integer type than the shifted value, in the
 * value's Wasm type (spec/05-expressions.md#shifts). An `i64` count is
 * range-checked before it narrows, and a signed count extends with its sign,
 * so a negative or oversized count still panics.
 */
export function shiftCount(
  expression: Extract<HirExpression, { kind: "binary" }>,
  count: string,
): string {
  if (expression.operator !== "<<" && expression.operator !== ">>") return count;
  const value = numericType(expression.left.type)?.wasm;
  const counted = numericType(expression.right.type);
  if (!value || !counted || counted.wasm === value) return count;
  if (value === "i64")
    return `(i64.extend_i32_${counted.family === "unsigned" ? "u" : "s"} ${count})`;
  return `(i32.wrap_i64 (call $hd.check_shift_i64 ${count}))`;
}
