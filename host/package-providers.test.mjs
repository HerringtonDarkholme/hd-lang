import assert from "node:assert/strict";
import { test } from "node:test";
import { createPackageRuntime, readPackageContract } from "./package-providers.mjs";
import { Dec, Enc, HostContract } from "./wire.mjs";

const runtime = () => createPackageRuntime({});
const encode = (codec, value) => {
  const e = new Enc();
  codec.enc(e, value, []);
  return e.done();
};
const roundTrip = (codec, value) => {
  const d = new Dec(encode(codec, value));
  const result = codec.dec(d);
  assert.equal(d.at, d.b.length);
  return result;
};

test("package codecs retain exact integer ranges, float widths and void", () => {
  const r = runtime();
  for (const bits of [8, 16, 32, 64]) {
    for (const signed of [false, true]) {
      const t = r.int(bits, signed);
      const low = signed ? -(1n << BigInt(bits - 1)) : 0n;
      const high = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
      for (const n of [low, 0n, high]) assert.equal(roundTrip(t, n), bits === 64 ? n : Number(n));
      for (const bad of [low - 1n, high + 1n, 1.1, NaN, Infinity, "1", null])
        assert.throws(() => encode(t, bad), HostContract);
    }
  }
  assert.equal(encode(r.f32, 1.25).length, 4);
  assert.equal(encode(r.f64, 1.25).length, 8);
  assert.equal(roundTrip(r.f32, 1 / 3), Math.fround(1 / 3));
  assert.ok(Object.is(roundTrip(r.f64, -0), -0));
  assert.ok(Number.isNaN(roundTrip(r.f32, NaN)));
  assert.equal(roundTrip(r.unit, undefined), undefined);
  assert.throws(() => encode(r.unit, null), HostContract);
  assert.throws(() => r.f64.dec(new Dec(new Uint8Array(7))), HostContract);
  assert.throws(() => encode(r.bool, 1), HostContract);
  assert.equal(roundTrip(r.bool, false), false);
});

test("package codecs own their data and preserve names including __proto__", () => {
  const r = runtime();
  const record = r.record([
    ["__proto__", r.str],
    ["number", r.int(64, false)],
  ]);
  const input = Object.fromEntries([
    ["__proto__", "literal data"],
    ["number", 18446744073709551615n],
  ]);
  const result = roundTrip(record, input);
  assert.deepEqual(result, input);
  assert.equal(Object.getPrototypeOf(result), Object.prototype);
  assert.throws(() => encode(record, { number: 1n }), HostContract);
  assert.deepEqual(roundTrip(r.bytes, new Uint8Array([0, 255])), new Uint8Array([0, 255]));
  assert.throws(() => encode(r.bytes, [1]), HostContract);
  assert.deepEqual(roundTrip(r.tuple([r.char, r.option(r.str)]), ["🦉", null]), ["🦉", null]);
  assert.throws(() => encode(r.char, "two"), HostContract);
  assert.throws(() => encode(r.char, "\ud800"), HostContract);
  assert.deepEqual(roundTrip(r.list(r.unit), [undefined, undefined]), [undefined, undefined]);
  const enumeration = r.enumeration([
    ["Empty", []],
    ["Broken", [["reason", r.str]]],
  ]);
  assert.deepEqual(roundTrip(enumeration, { tag: "Broken", fields: { reason: "why" } }), {
    tag: "Broken",
    fields: { reason: "why" },
  });
  assert.throws(() => encode(enumeration, { tag: "Other" }), HostContract);
  const res = r.result(r.str, enumeration);
  assert.deepEqual(roundTrip(res, { ok: false, error: { tag: "Empty", fields: {} } }), {
    ok: false,
    error: { tag: "Empty", fields: {} },
  });
  const withTag = r.enumeration([["Event", [["tag", r.int(32, false)]]]]);
  assert.deepEqual(roundTrip(withTag, { tag: "Event", fields: { tag: 7 } }), {
    tag: "Event",
    fields: { tag: 7 },
  });
  assert.throws(() => encode(res, { ok: 0, error: {} }), HostContract);
  const privateText = '{"large":18446744073709551615,"custom":"representation"}';
  assert.equal(roundTrip(r.json, privateText), privateText);
  assert.throws(() => encode(r.json, JSON.parse(privateText)), HostContract);
});

test("package contract reader rejects truncated, noncanonical and unknown versions", () => {
  const empty = new Uint8Array([1, 0, 0, 0, 0]);
  assert.deepEqual(readPackageContract(empty), []);
  for (let n = 0; n < empty.length; n++)
    assert.throws(() => readPackageContract(empty.subarray(0, n)), HostContract);
  for (const bytes of [
    [2, 0, 0, 0, 0],
    [1, 0, 0, 0, 128, 0],
    [1, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 255, 255, 255, 255, 15],
  ])
    assert.throws(() => readPackageContract(new Uint8Array(bytes)), HostContract);
});
