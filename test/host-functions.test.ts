import assert from "node:assert/strict";
import test from "node:test";

import { HOST_FUNCTIONS } from "../src/host-functions.ts";

function hostNumber(name: string, ...arguments_: Array<number | string>): number {
  const result = HOST_FUNCTIONS[name]?.(...arguments_);
  assert.ok(typeof result === "number");
  return result;
}

function hostString(name: string, ...arguments_: Array<number | string>): string {
  const result = HOST_FUNCTIONS[name]?.(...arguments_);
  assert.ok(typeof result === "string");
  return result;
}

function f64FromBits(bits: bigint): number {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setBigUint64(0, bits, false);
  return view.getFloat64(0, false);
}

test("parse_f64 accepts every unsigned decimal form used by std", () => {
  assert.equal(hostNumber("parse_f64", "1.5"), 1.5);
  assert.equal(hostNumber("parse_f64", ".5"), 0.5);
  assert.equal(hostNumber("parse_f64", "5."), 5);
  assert.equal(hostNumber("parse_f64", "007"), 7);
  assert.equal(hostNumber("parse_f64", "1E+5"), 100_000);
  assert.equal(hostNumber("parse_f64", "1e-5"), 0.000_01);
});

test("parse_f64 rounds exact decimal ties to an even binary significand", () => {
  // Exactly halfway from 1.0 to the next f64. The long form also makes this
  // independent of the host engine's permitted approximation after digit 20.
  assert.equal(
    hostNumber("parse_f64", "1.00000000000000011102230246251565404236316680908203125"),
    1,
  );
  assert.equal(
    hostNumber("parse_f64", "1.00000000000000011102230246251565404236316680908203126"),
    f64FromBits(0x3ff0000000000001n),
  );
  // Here the lower significand is odd, so the equally near upper value wins.
  assert.equal(
    hostNumber("parse_f64", "1.00000000000000033306690738754696212708950042724609375"),
    f64FromBits(0x3ff0000000000002n),
  );
  assert.equal(hostNumber("parse_f64", "9007199254740993"), 9_007_199_254_740_992);
});

test("parse_f64 rounds at the finite, subnormal, and zero boundaries", () => {
  assert.equal(hostNumber("parse_f64", "1.7976931348623157e308"), Number.MAX_VALUE);
  assert.equal(hostNumber("parse_f64", "1.7976931348623158e308"), Number.MAX_VALUE);
  assert.equal(hostNumber("parse_f64", "1.7976931348623159e308"), Infinity);
  // The overflow threshold is halfway from the largest finite value to the
  // conceptual even significand at 2^1024.
  const overflowTie = ((1n << 54n) - 1n) << 970n;
  assert.equal(hostNumber("parse_f64", String(overflowTie - 1n)), Number.MAX_VALUE);
  assert.equal(hostNumber("parse_f64", String(overflowTie)), Infinity);
  assert.equal(hostNumber("parse_f64", "5e-324"), Number.MIN_VALUE);

  // 2^-1075 is exactly 5^1075 * 10^-1075.
  const halfCoefficient = 5n ** 1075n;
  const halfMinimum = `${halfCoefficient}e-1075`;
  assert.equal(hostNumber("parse_f64", halfMinimum), 0);
  assert.equal(hostNumber("parse_f64", `${halfCoefficient + 1n}e-1075`), Number.MIN_VALUE);

  // Halfway from the largest subnormal to the smallest normal, whose even
  // significand wins.
  const normalTie = ((1n << 53n) - 1n) * 5n ** 1075n;
  assert.equal(
    hostNumber("parse_f64", `${normalTie - 1n}e-1075`),
    f64FromBits(0x000fffffffffffffn),
  );
  assert.equal(hostNumber("parse_f64", `${normalTie}e-1075`), f64FromBits(0x0010000000000000n));
  assert.equal(
    hostNumber("parse_f64", `${normalTie + 1n}e-1075`),
    f64FromBits(0x0010000000000000n),
  );
});

test("parse_f64 handles huge exponents without constructing huge powers", () => {
  const huge = "9".repeat(10_000);
  assert.equal(hostNumber("parse_f64", `1e${huge}`), Infinity);
  assert.equal(hostNumber("parse_f64", `1e-${huge}`), 0);
  assert.ok(!Object.is(hostNumber("parse_f64", `1e-${huge}`), -0));
  assert.equal(hostNumber("parse_f64", `0e${huge}`), 0);
});

test("parse_f64 validates the whole exponent after its magnitude saturates", () => {
  const huge = "9".repeat(10_000);
  assert.throws(() => hostNumber("parse_f64", `1e${huge}x`), /invalid decimal exponent/);
  assert.throws(() => hostNumber("parse_f64", `1e-${huge}x`), /invalid decimal exponent/);
  assert.throws(() => hostNumber("parse_f64", "1e+"), /invalid decimal exponent/);
});

test("format_f64_fixed rounds the exact binary value with ties to even", () => {
  assert.equal(hostString("format_f64_fixed", 3.14159, 2), "3.14");
  assert.equal(hostString("format_f64_fixed", 0.1, 20), "0.10000000000000000555");
  assert.equal(hostString("format_f64_fixed", 2.5, 0), "2");
  assert.equal(hostString("format_f64_fixed", 3.5, 0), "4");
  assert.equal(hostString("format_f64_fixed", 0.125, 2), "0.12");
  assert.equal(hostString("format_f64_fixed", -1.5, 0), "-2");
});

test("format_f64_fixed preserves fixed notation, zero padding, and the sign bit", () => {
  assert.equal(hostString("format_f64_fixed", 7, 0), "7");
  assert.equal(hostString("format_f64_fixed", 0.5, 3), "0.500");
  assert.equal(hostString("format_f64_fixed", 1e21, 1), "1000000000000000000000.0");
  assert.equal(hostString("format_f64_fixed", 1e-7, 3), "0.000");
  assert.equal(hostString("format_f64_fixed", -0, 1), "-0.0");
  assert.equal(hostString("format_f64_fixed", -0.001, 2), "-0.00");
  assert.equal(hostString("format_f64_fixed", Number.MIN_VALUE, 100), `0.${"0".repeat(100)}`);
  assert.doesNotMatch(hostString("format_f64_fixed", Number.MAX_VALUE, 1), /e/i);
});

test("format_f64_fixed writes special values and rejects an invalid digit count", () => {
  assert.equal(hostString("format_f64_fixed", NaN, 2), "NaN");
  assert.equal(hostString("format_f64_fixed", Infinity, 2), "inf");
  assert.equal(hostString("format_f64_fixed", -Infinity, 2), "-inf");
  assert.throws(() => HOST_FUNCTIONS.format_f64_fixed?.(1, -1));
  assert.throws(() => HOST_FUNCTIONS.format_f64_fixed?.(1, 101));
  assert.throws(() => HOST_FUNCTIONS.format_f64_fixed?.(1, 1.5));
});
