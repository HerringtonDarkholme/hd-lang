import assert from "node:assert/strict";
import test from "node:test";
import { compilerImports } from "../src/compiler-host.ts";

test("NFC uses a sized owned output, including expansion and memory growth", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2 });
  const imports = compilerImports(() => memory);
  const normalize = imports.hd_web!.nfc_utf8 as (
    input: number,
    length: number,
    output: number,
    capacity: number,
  ) => number;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  for (const text of ["é", "e\u0301", "\u0344", "\u1100\u1161", "λ", "a\0b"]) {
    const input = encoder.encode(text);
    new Uint8Array(memory.buffer, 16, input.length).set(input);
    const size = normalize(16, input.length, 0, 0);
    const expected = encoder.encode(text.normalize("NFC"));
    assert.equal(size, expected.length);
    const sentinel = new Uint8Array(memory.buffer, 1024, size);
    sentinel.fill(0xaa);
    if (size > 1) {
      assert.equal(normalize(16, input.length, 1024, size - 1), size);
      assert.ok(
        sentinel.every((byte) => byte === 0xaa),
        "an insufficient buffer is not written",
      );
    }
    if (text === "e\u0301") memory.grow(1);
    assert.equal(normalize(16, input.length, 1024, size), size);
    assert.equal(decoder.decode(new Uint8Array(memory.buffer, 1024, size)), text.normalize("NFC"));
  }
});
