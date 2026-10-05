import type { ValueType } from "./hir.ts";
import { numericType } from "./numeric.ts";

// The scalar values of a host capability call (src/compiler.ts): a number
// for a narrow scalar, a BigInt for a 64-bit integer, and a JavaScript
// string for a `string`. An argument from Wasm is normalized; a result from
// the host is only checked (spec/lang/10-modules.md#host-results).

type HostSuspensionValue = number | bigint | string;

/** Whether a JavaScript string contains only complete Unicode scalar values. */
function isWellFormedText(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (index + 1 >= value.length || next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

/**
 * Decode a value that Wasm passed to the host. Narrow integers arrive as an
 * i32 bit pattern, so unsigned arguments need normalization before the host
 * sees them. This is deliberately separate from checking values returned by
 * the host: results must never be rounded, truncated, or wrapped to fit.
 */
export function hostArgumentValue(
  type: ValueType,
  value: HostSuspensionValue,
): HostSuspensionValue {
  if (type === "string") {
    if (typeof value !== "string") throw new Error("host string boundary value must be a string");
    return value;
  }
  const numeric = numericType(type);
  if (numeric?.wasm === "i64") {
    if (typeof value !== "bigint") throw new Error(`host ${type} boundary value must be a BigInt`);
    return numeric.family === "unsigned" ? BigInt.asUintN(64, value) : BigInt.asIntN(64, value);
  }
  if (numeric?.family === "float") {
    if (typeof value !== "number") throw new Error(`host ${type} boundary value must be a number`);
    return value;
  }
  if (typeof value !== "number") throw new Error(`host ${type} boundary value must be a number`);
  if (type === "bool") return value === 0 ? 0 : 1;
  if (numeric?.family === "unsigned") return value >>> 0;
  return value | 0;
}

/** Validate one scalar supplied by the host without changing it. */
export function checkedHostValue(type: ValueType, value: HostSuspensionValue): HostSuspensionValue {
  if (type === "string") {
    if (typeof value !== "string") throw new Error("expected a string");
    if (!isWellFormedText(value)) throw new Error("expected valid Unicode text");
    return value;
  }
  if (type === "bool") {
    if (typeof value !== "number" || (value !== 0 && value !== 1))
      throw new Error("expected bool as 0 or 1");
    return value;
  }
  if (type === "char") {
    if (
      typeof value !== "number" ||
      !Number.isInteger(value) ||
      value < 0 ||
      value > 0x10ffff ||
      (value >= 0xd800 && value <= 0xdfff)
    )
      throw new Error("expected a Unicode scalar value");
    return value;
  }
  const numeric = numericType(type);
  if (numeric?.family === "float") {
    if (typeof value !== "number") throw new Error(`expected a ${type} number`);
    if (type === "f32" && !Object.is(Math.fround(value), value))
      throw new Error(`expected a value already representable as f32, received ${value}`);
    // All values of the declared width are valid, including NaN, infinities,
    // and -0.0. In particular, validation never uses finiteness as a proxy.
    return value;
  }
  if (numeric?.wasm === "i64") {
    if (typeof value !== "bigint") throw new Error(`expected ${type} as a BigInt`);
    if (value < numeric.minimum! || value > numeric.maximum!)
      throw new Error(
        `expected ${type} in ${numeric.minimum}..${numeric.maximum}, received ${value}`,
      );
    return value;
  }
  if (numeric) {
    if (typeof value !== "number" || !Number.isInteger(value))
      throw new Error(`expected an integer ${type}`);
    const integer = BigInt(value);
    if (integer < numeric.minimum! || integer > numeric.maximum!)
      throw new Error(
        `expected ${type} in ${numeric.minimum}..${numeric.maximum}, received ${value}`,
      );
    return value;
  }
  throw new Error(`the host cannot build a '${type}' boundary value`);
}
