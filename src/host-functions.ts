// The host side of the generic host-function boundary (src/README.md,
// Compiler/library boundary). A `lib/std` function with an
// `@intrinsic("name")` line that is not a runtime primitive
// (emitter/intrinsics.ts) is imported as `host:<name>` and runs the entry
// here. Arguments and results are boundary values: numbers for the scalar
// types (`bool` as 0 or 1, `char` as its code point, `i64` as a BigInt) and
// JavaScript strings for `string`. Adding a host-backed std function needs
// its `lib/std` declaration and one entry here, nothing in the compiler.

import type { HostSuspensionCall, HostSuspensionOutcome } from "./compiler.ts";
import { RuntimePanicError, runtimePanicCategory } from "./runtime-panic.ts";

type HostFunctionValue = number | bigint | string;

export type HostFunction = (...arguments_: HostFunctionValue[]) => HostFunctionValue | void;

export const HOST_FUNCTIONS: Readonly<Record<string, HostFunction>> = {
  // Unicode Default Case Conversion with full mappings and no locale
  // (spec/std/text.md#r-std-text.string.lower).
  string_lower: (text) => String(text).toLowerCase(),
  string_upper: (text) => String(text).toUpperCase(),
  // A float's `Display` text (spec/lang/04-type-system.md#numeric-display).
  format_f64: (value) => displayF64(Number(value)),
  format_f32: (value) => displayF32(Number(value)),
  // Decimal conversion stays exact at the host boundary. `lib/std` checks
  // the source grammar and the fixed digit range before it calls these.
  parse_f64: (text) => parseF64(String(text)),
  format_f64_fixed: (value, digits) => formatF64Fixed(Number(value), Number(digits)),
  // The one panic primitive of `lib/std` (spec/std/README.md#standard-library-primitives):
  // a checked runtime panic of a stable category that shows `message`, such
  // as a failed `assert`'s `assertion-failed`
  // (spec/lang/10-modules.md#r-module.testing.assert-equal-debug). An empty
  // message shows none, as for `index-out-of-bounds`.
  panic: (category, message) => {
    throw new RuntimePanicError(
      runtimePanicCategory(String(category)),
      String(message) === "" ? undefined : String(message),
    );
  },
};

/**
 * An f32 shows the shortest decimal that rounds back to the same f32, in the
 * f64 notation (spec/lang/04-type-system.md#numeric-display).
 */
function displayF32(value: number): string {
  if (!Number.isFinite(value)) return displayF64(value);
  for (let digits = 1; digits <= 9; digits += 1) {
    const shortest = Number(value.toPrecision(digits));
    if (Math.fround(shortest) === value) return displayF64(Object.is(value, -0) ? -0 : shortest);
  }
  return displayF64(value);
}

function displayF64(value: number): string {
  if (Number.isNaN(value)) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  if (Object.is(value, -0)) return "-0.0";
  const rendered = value.toString();
  return !rendered.includes(".") && !rendered.includes("e") ? `${rendered}.0` : rendered;
}

const F64_FRACTION_BITS = 52n;
const F64_FRACTION_MASK = (1n << F64_FRACTION_BITS) - 1n;
const F64_HIDDEN_BIT = 1n << F64_FRACTION_BITS;
const F64_EXPONENT_BIAS = 1023;

/** The nearest integer to `numerator / denominator`, with ties to even. */
function roundedQuotient(numerator: bigint, denominator: bigint): bigint {
  const quotient = numerator / denominator;
  const twiceRemainder = (numerator % denominator) * 2n;
  return twiceRemainder > denominator || (twiceRemainder === denominator && quotient % 2n === 1n)
    ? quotient + 1n
    : quotient;
}

/** `floor(log2(numerator / denominator))` for two positive integers. */
function binaryExponent(numerator: bigint, denominator: bigint): number {
  let exponent = numerator.toString(2).length - denominator.toString(2).length;
  const below =
    exponent >= 0
      ? numerator < denominator << BigInt(exponent)
      : numerator << BigInt(-exponent) < denominator;
  if (below) exponent -= 1;
  return exponent;
}

function f64FromBits(bits: bigint): number {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setBigUint64(0, bits, false);
  return view.getFloat64(0, false);
}

function f64Bits(value: number): bigint {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setFloat64(0, value, false);
  return view.getBigUint64(0, false);
}

/**
 * Read an exponent without constructing an unbounded integer. Once its
 * magnitude exceeds the source length plus the whole f64 decimal range, it
 * can only force overflow or underflow.
 */
function decimalExponent(text: string, limit: number): number {
  let index = 0;
  let sign = 1;
  if (text[index] === "+" || text[index] === "-") {
    if (text[index] === "-") sign = -1;
    index += 1;
  }
  if (index === text.length) throw new Error("parse_f64 received an invalid decimal exponent");
  let value = 0;
  let saturated = false;
  for (; index < text.length; index += 1) {
    const digit = text.charCodeAt(index) - 48;
    if (digit < 0 || digit > 9) throw new Error("parse_f64 received an invalid decimal exponent");
    if (!saturated) {
      value = value * 10 + digit;
      if (value > limit) {
        value = limit;
        saturated = true;
      }
    }
  }
  return sign * value;
}

/**
 * The correctly rounded binary64 value of an unsigned decimal. The std
 * caller owns the grammar and sign; doing the conversion from an exact
 * rational avoids a host engine's permitted approximation after the
 * twentieth significant decimal digit.
 */
function parseF64(text: string): number {
  const exponentAt = text.search(/[eE]/);
  const mantissaEnd = exponentAt < 0 ? text.length : exponentAt;
  const mantissa = text.slice(0, mantissaEnd);
  const point = mantissa.indexOf(".");
  if (point !== -1 && point !== mantissa.lastIndexOf("."))
    throw new Error("parse_f64 received an invalid decimal");
  const whole = point < 0 ? mantissa : mantissa.slice(0, point);
  const fraction = point < 0 ? "" : mantissa.slice(point + 1);
  if (
    (whole.length === 0 && fraction.length === 0) ||
    !/^\d*$/.test(whole) ||
    !/^\d*$/.test(fraction)
  )
    throw new Error("parse_f64 received an invalid decimal");
  if (exponentAt >= 0 && /[eE]/.test(text.slice(exponentAt + 1)))
    throw new Error("parse_f64 received an invalid decimal exponent");

  const explicitExponent =
    exponentAt < 0 ? 0 : decimalExponent(text.slice(exponentAt + 1), text.length + 400);
  let coefficient = `${whole}${fraction}`.replace(/^0+/, "");
  if (coefficient === "") return 0;
  let power = explicitExponent - fraction.length;
  const adjustedExponent = coefficient.length - 1 + power;
  if (adjustedExponent > 308) return Infinity;
  if (adjustedExponent < -324) return 0;

  // Removing decimal zeroes keeps the exact rational smaller.
  const trailingZeroes = coefficient.match(/0+$/)?.[0].length ?? 0;
  if (trailingZeroes > 0) {
    coefficient = coefficient.slice(0, -trailingZeroes);
    power += trailingZeroes;
  }
  let numerator = BigInt(coefficient);
  let denominator = 1n;
  if (power >= 0) numerator *= 10n ** BigInt(power);
  else denominator = 10n ** BigInt(-power);

  let exponent = binaryExponent(numerator, denominator);
  if (exponent > 1023) return Infinity;
  let bits: bigint;
  if (exponent >= -1022) {
    const shift = 52 - exponent;
    let significand =
      shift >= 0
        ? roundedQuotient(numerator << BigInt(shift), denominator)
        : roundedQuotient(numerator, denominator << BigInt(-shift));
    if (significand === 1n << 53n) {
      significand >>= 1n;
      exponent += 1;
    }
    if (exponent > 1023) return Infinity;
    bits =
      (BigInt(exponent + F64_EXPONENT_BIAS) << F64_FRACTION_BITS) | (significand - F64_HIDDEN_BIT);
  } else {
    const significand = roundedQuotient(numerator << 1074n, denominator);
    if (significand === 0n) return 0;
    bits = significand;
  }
  return f64FromBits(bits);
}

/** Fixed-point text produced from the exact binary value, with ties to even. */
function formatF64Fixed(value: number, digits: number): string {
  if (!Number.isInteger(digits) || digits < 0 || digits > 100)
    throw new Error("format_f64_fixed digits must be from 0 to 100");
  if (Number.isNaN(value)) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";

  const negative = value < 0 || Object.is(value, -0);
  const bits = f64Bits(Math.abs(value));
  const storedExponent = Number((bits >> F64_FRACTION_BITS) & 0x7ffn);
  const fraction = bits & F64_FRACTION_MASK;
  const significand = storedExponent === 0 ? fraction : F64_HIDDEN_BIT | fraction;
  const exponent = storedExponent === 0 ? -1074 : storedExponent - F64_EXPONENT_BIAS - 52;

  // Multiplication by 10^digits is 5^digits followed by a binary shift.
  let rounded = significand * 5n ** BigInt(digits);
  const shift = exponent + digits;
  if (shift >= 0) rounded <<= BigInt(shift);
  else rounded = roundedQuotient(rounded, 1n << BigInt(-shift));

  const decimal = rounded.toString();
  let result: string;
  if (digits === 0) result = decimal;
  else if (decimal.length <= digits) result = `0.${"0".repeat(digits - decimal.length)}${decimal}`;
  else result = `${decimal.slice(0, -digits)}.${decimal.slice(-digits)}`;
  return negative ? `-${result}` : result;
}

/** What a built-in host provider may use from the embedder. */
interface HostProviderContext {
  readonly console?: (text: string, provider: unknown) => void;
  readonly consoleError?: (text: string, provider: unknown) => void;
}

/**
 * Built-in implementations of host capability methods, keyed
 * `Trait.method`. Each call arrives through the generic per-method bridge
 * (emitter/host-providers.ts) like any host capability's.
 */
export const HOST_PROVIDERS: Readonly<
  Record<string, (call: HostSuspensionCall, host: HostProviderContext) => HostSuspensionOutcome>
> = {
  // The host console (spec/lang/10-modules.md#console): `write_line!` writes its
  // line when first polled and is then ready with `.Ok(())`. The host reports
  // no write failure, so it never builds a `ConsoleError`.
  "Console.write_line": (call, host) => {
    host.console?.(String(call.arguments[0]), call.provider);
    return { pending: false, value: { tag: "ok" } };
  },
  "Console.write_error_line": (call, host) => {
    host.consoleError?.(String(call.arguments[0]), call.provider);
    return { pending: false, value: { tag: "ok" } };
  },
};

/**
 * Host capabilities whose calls are neither recorded nor replayed: `hd
 * replay` writes console lines again rather than reading them back. Whether
 * a replay should capture them is an owner question
 * (future-work/OPEN_ISSUES.md, Mutable Host Providers).
 */
export const UNRECORDED_PROVIDERS: ReadonlySet<string> = new Set(["Console"]);

interface StringHandle {
  readonly bytes: Uint8Array;
}

/**
 * The generic host-function boundary: a `string` crosses as a handle whose
 * UTF-8 bytes the Wasm side copies one by one. Through it, `entry_error`
 * takes the report of an entry point's `.Err`, which the host writes to
 * standard error (spec/lang/10-modules.md#r-module.entry.err-stderr), or of a
 * test case's, which `hd test` puts in its own report
 * (r-module.testing.err-print; emitter/context.ts, emitEntryReport).
 */
export function hostStringImports(
  writeEntryError: ((report: string, provider: unknown) => void) | undefined,
): Record<string, (...arguments_: unknown[]) => unknown> {
  const decoder = new TextDecoder();
  return {
    host_string_new: (length) => ({ bytes: new Uint8Array(Number(length)) }),
    host_string_set: (handle, index, byte) => {
      (handle as StringHandle).bytes[Number(index)] = Number(byte);
    },
    host_string_length: (handle) => (handle as StringHandle).bytes.length,
    host_string_get: (handle, index) => (handle as StringHandle).bytes[Number(index)],
    entry_error: (handle) =>
      writeEntryError?.(decoder.decode((handle as StringHandle).bytes), undefined),
  };
}
