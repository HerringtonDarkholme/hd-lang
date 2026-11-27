import assert from "node:assert/strict";
import { closeSync, openSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import type { TestContext } from "node:test";

import { lineReader } from "./node.ts";
import { fixture } from "./node-test-support.ts";

function reader(context: TestContext, content: Uint8Array) {
  const root = fixture(context, { input: content });
  const fd = openSync(join(root, "input"), "r");
  context.after(() => closeSync(fd));
  return lineReader(fd);
}

test("stdin joins and searches at most one input length at 1 and 10 MiB", (context) => {
  let copied = 0;
  let searched = 0;
  const concat = Buffer.concat;
  const indexOf = Buffer.prototype.indexOf;
  context.mock.method(Buffer, "concat", (...args: Parameters<typeof Buffer.concat>) => {
    copied += args[1] ?? args[0].reduce((sum, bytes) => sum + bytes.length, 0);
    return concat(...args);
  });
  context.mock.method(
    Buffer.prototype,
    "indexOf",
    function (this: Buffer, ...args: Parameters<typeof Buffer.prototype.indexOf>) {
      const found = indexOf.apply(this, args);
      searched += found < 0 ? this.length : found + 1;
      return found;
    },
  );
  for (const size of [1024 * 1024, 10 * 1024 * 1024]) {
    for (const mode of ["line", "all"] as const) {
      const bytes = Buffer.alloc(size, 120);
      const input = reader(context, bytes);
      copied = 0;
      searched = 0;
      const result = mode === "line" ? input() : input.all();
      assert.equal(typeof result, "string");
      assert.equal((result as string).length, size);
      assert.ok(copied <= size, `${mode}: copied ${copied} bytes for ${size} input bytes`);
      assert.equal(searched, mode === "line" ? size : 0);
      assert.equal(input(), null);
      assert.equal(input.all(), "");
    }
  }
});

test("stdin preserves UTF-8 and CRLF across chunks and mixed line/all reads", (context) => {
  // Split the multibyte character across one fill and CRLF across another.
  const prefix = "x".repeat(65535);
  const middle = "y".repeat(65534);
  const input = reader(context, Buffer.from(`${prefix}é${middle}\r\nsecond\r\nlast\r`));
  assert.equal(input(), `${prefix}é${middle}`);
  assert.equal(input.all(), "second\r\nlast\r");
  assert.equal(input(), null);
  assert.equal(input.all(), "");
});

test("stdin preserves empty lines, closed input and descriptor failures", (context) => {
  const input = reader(context, Buffer.from("\n\r\nlast"));
  assert.equal(input(), "");
  assert.equal(input(), "");
  assert.equal(input(), "last");
  assert.equal(input(), null);
  const closed = lineReader(null);
  assert.equal(closed(), null);
  assert.equal(closed.all(), "");
  const failed = lineReader(-1);
  assert.equal(failed(), false);
  assert.equal(failed.all(), false);
});
