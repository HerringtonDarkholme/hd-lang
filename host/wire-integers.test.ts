import assert from "node:assert/strict";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { Dec, Enc, signed, signed64, unsigned, unsigned64 } from "./wire.ts";

const MAX_U64 = (1n << 64n) - 1n;

function check(value: number | bigint, zigzag = false): void {
  const expected = BigInt(value);
  const encoder = new Enc();
  if (zigzag) {
    encoder.zz(value);
  } else {
    encoder.leb(value);
  }
  const bytes = encoder.done();
  const decoder = new Dec(bytes);
  assert.equal(zigzag ? decoder.zz() : decoder.leb(), expected);
  assert.equal(decoder.at, bytes.length);
  let encoded = expected;
  if (zigzag) {
    encoded = expected >= 0n ? expected * 2n : -expected * 2n - 1n;
  }
  assert.equal(bytes.length, Math.ceil(encoded.toString(2).length / 7));
}

test("integer writers retain the canonical boundary bytes", () => {
  const cases: [number | bigint, number[]][] = [
    [0, [0]],
    [127, [127]],
    [128, [128, 1]],
    [16383, [255, 127]],
    [16384, [128, 128, 1]],
    [0xffffffff, [255, 255, 255, 255, 15]],
    [Number.MAX_SAFE_INTEGER, [255, 255, 255, 255, 255, 255, 255, 15]],
    [1n << 53n, [128, 128, 128, 128, 128, 128, 128, 16]],
    [MAX_U64, [255, 255, 255, 255, 255, 255, 255, 255, 255, 1]],
  ];
  for (const [value, expected] of cases) {
    assert.deepEqual([...new Enc().leb(value).done()], expected);
  }
  assert.deepEqual([...new Enc().zz(-(1n << 63n)).done()], cases.at(-1)?.[1]);
  assert.deepEqual(
    [...new Enc().zz((1n << 63n) - 1n).done()],
    [254, 255, 255, 255, 255, 255, 255, 255, 255, 1],
  );
});

test("Number and BigInt writers agree around every 64-bit boundary", () => {
  check(0);
  check(-0);
  check(0, true);
  check(-0, true);
  for (let bit = 0n; bit <= 64n; bit++) {
    for (const offset of [-2n, -1n, 0n, 1n, 2n]) {
      const value = (1n << bit) + offset;
      if (value < 0n || value > MAX_U64) {
        continue;
      }
      check(value);
      if (Number.isSafeInteger(Number(value))) {
        check(Number(value));
      }
      const signedValue = value >> 1n;
      check(signedValue, true);
      check(-signedValue, true);
      if (Number.isSafeInteger(Number(signedValue))) {
        check(Number(signedValue), true);
        check(-Number(signedValue), true);
      }
    }
  }
  for (const value of [2 ** 53, 2 ** 60, 2 ** 63]) {
    check(value);
    check(value, true);
    check(-value, true);
  }
});

test("mixed integer fields survive buffer growth and keep earlier views alive", () => {
  const held = new Enc().leb(7).done();
  const encoder = new Enc();
  for (let value = 0; value < 65536; value++) {
    unsigned.enc(encoder, value);
    signed.enc(encoder, value - 32768);
    unsigned64.enc(encoder, MAX_U64 - BigInt(value));
    signed64.enc(encoder, BigInt(value) - (1n << 63n));
  }
  const bytes = encoder.done();
  const decoder = new Dec(bytes);
  for (let value = 0; value < 65536; value++) {
    assert.equal(unsigned.dec(decoder), value);
    assert.equal(signed.dec(decoder), value - 32768);
    assert.equal(unsigned64.dec(decoder), MAX_U64 - BigInt(value));
    assert.equal(signed64.dec(decoder), BigInt(value) - (1n << 63n));
  }
  assert.equal(decoder.at, bytes.length);
  assert.deepEqual([...held], [7]);
});

function bigIntCalls(context: TestContext): () => number {
  const native = BigInt;
  let calls = 0;
  context.after(() => {
    globalThis.BigInt = native;
  });
  globalThis.BigInt = new Proxy(native, {
    apply(target, self, args) {
      calls++;
      return Reflect.apply(target, self, args);
    },
  });
  return () => calls;
}

for (const count of [1000, 10000]) {
  test(`exact integer encoding needs zero BigInt conversions at ${count} four-field messages`, (context) => {
    const small = Array.from({ length: 256 }, (_, value) => BigInt(value));
    const negative = small.map((value) => -value);
    const calls = bigIntCalls(context);
    const encoder = new Enc();
    for (let index = 0; index < count; index++) {
      const value = index & 255;
      encoder
        .leb(value)
        .zz(value - 128)
        .leb(small[value])
        .zz(negative[value]);
    }
    assert.equal(calls(), 0);
    assert.ok(encoder.done().length >= count * 4);
  });
  test(`wide integer fallback stays exact at ${count} pairs`, (context) => {
    const calls = bigIntCalls(context);
    const encoder = new Enc();
    for (let index = 0; index < count; index++) {
      encoder.leb(MAX_U64).zz(-(1n << 63n));
    }
    assert.equal(calls(), count * 2);
    assert.equal(encoder.done().length, count * 20);
  });
}
