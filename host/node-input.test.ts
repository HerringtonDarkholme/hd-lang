import assert from "node:assert/strict";
import { closeSync, openSync, writeSync, unlinkSync, fstatSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { join } from "node:path";
import { test } from "node:test";
import { constants as osConstants } from "node:os";
import type { TestContext } from "node:test";

import { createGrant, lineReader, nodeLayer } from "./node.ts";
import { fixture, netWorker } from "./node-test-support.ts";

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

test("stdin byte reads preserve CRLF, invalid UTF-8, EOF and mixed text reads", (context) => {
  const input = reader(context, Buffer.from([97, 13, 10, 255, 0, 13, 10, 98, 13, 10, 99, 13]));
  assert.deepEqual(input.bytes(0), new Uint8Array());
  assert.deepEqual([...(input.bytes(1) as Uint8Array)], [97]);
  assert.equal(input(), "");
  assert.deepEqual([...(input.bytes(4) as Uint8Array)], [255, 0, 13, 10]);
  assert.equal(input.all(), "b\r\nc\r");
  assert.deepEqual([...(input.bytes(100) as Uint8Array)], []);
  assert.deepEqual(lineReader(null).bytes(100), new Uint8Array());
  assert.equal(lineReader(-1).bytes(100), false);
});

test("closed stdin provider returns byte EOF without synthesizing a line", (context) => {
  const platform = nodeLayer.providers({
    env: { stdin: null },
    sink: { out() {}, err() {} },
    st: {},
    grant: createGrant(),
    flush() {},
    left: () => Infinity,
  });
  context.after(() => platform.close());
  const input = platform.traits["hd:ConsoleInput"];
  assert.deepEqual(input["StdinHandle.read"](input.stdin(), 100), {
    ok: true,
    value: new Uint8Array(),
  });
  assert.deepEqual(input.read_line(), { ok: true, value: null });
  assert.deepEqual(input.read_all(), { ok: true, value: "" });
});

async function inputResult(call: ReturnType<typeof netWorker>, operation: number) {
  const deadline = performance.now() + 1500;
  while (performance.now() < deadline) {
    const result = call({ op: "wait_poll", operation });
    if (!result.pending) {
      return result;
    }
    await delay(5);
  }
  assert.fail("FIFO standard input did not reach EOF before its deadline");
}

function fifoInput(
  context: TestContext,
  options: { manualClose?: boolean; unlink?: boolean } = {},
) {
  const root = fixture(context, {});
  const path = join(root, "stdin");
  // cost: one bounded mkfifo process for this special-file regression.
  execFileSync("mkfifo", [path], { timeout: 1000 });
  const rendezvous = openSync(path, "r+");
  const fd = openSync(path, "r");
  const identity = fstatSync(fd);
  let writer: number | null = openSync(path, "w");
  closeSync(rendezvous);
  if (options.unlink) {
    unlinkSync(path);
  }
  const call = netWorker(context, {
    url: new URL("./input.ts", import.meta.url),
    ...(options.manualClose ? {} : { close: "close" }),
  });
  const end = () => {
    if (writer !== null) {
      closeSync(writer);
      writer = null;
    }
  };
  context.after(() => {
    end();
    try {
      const current = fstatSync(fd);
      if (current.dev === identity.dev && current.ino === identity.ino) {
        closeSync(fd);
      }
    } catch (error) {
      assert.equal((error as NodeJS.ErrnoException).code, "EBADF");
    }
  });
  return {
    call,
    fd,
    end,
    write: (bytes: Uint8Array) => {
      assert.notEqual(writer, null);
      writeSync(writer as number, bytes);
    },
  };
}

test(
  "FIFO stdin restores a byte reservation and read_all reaches EOF",
  {
    skip: process.platform === "win32",
    timeout: 5000,
  },
  async (context) => {
    const { call, fd, write, end } = fifoInput(context, { unlink: true });
    const bytes = Buffer.from("hello\r\nlast");
    write(bytes);
    const first = call({ op: "wait_start", fd, kind: "bytes", max: 1 }).ok;
    assert.deepEqual(await inputResult(call, first), { ok: new Uint8Array([104]) });
    call({ op: "wait_abort", operation: first });
    const all = call({ op: "wait_start", fd, kind: "all" }).ok;
    assert.deepEqual(call({ op: "wait_poll", operation: all }), { pending: true });
    end();
    assert.deepEqual(await inputResult(call, all), { ok: bytes.toString("utf8") });
    call({ op: "wait_finish", operation: all });
    const eof = call({ op: "wait_start", fd, kind: "bytes", max: 2 }).ok;
    assert.deepEqual(await inputResult(call, eof), { ok: new Uint8Array() });
    call({ op: "wait_finish", operation: eof });
  },
);

test(
  "FIFO stdin byte chunks preserve binary data and report EOF",
  {
    skip: process.platform === "win32",
    timeout: 5000,
  },
  async (context) => {
    const { call, fd, write, end } = fifoInput(context);
    const zero = call({ op: "wait_start", fd, kind: "bytes", max: 0 }).ok;
    assert.deepEqual(await inputResult(call, zero), { ok: new Uint8Array() });
    call({ op: "wait_finish", operation: zero });
    const bytes = new Uint8Array([255, 13, 10, 0, 195, 169, 122]);
    write(bytes);
    for (let at = 0; at < bytes.length; at += 3) {
      const operation = call({ op: "wait_start", fd, kind: "bytes", max: 3 }).ok;
      assert.deepEqual(await inputResult(call, operation), { ok: bytes.slice(at, at + 3) });
      call({ op: "wait_finish", operation });
    }
    const eof = call({ op: "wait_start", fd, kind: "bytes", max: 3 }).ok;
    assert.deepEqual(call({ op: "wait_poll", operation: eof }), { pending: true });
    end();
    assert.deepEqual(await inputResult(call, eof), { ok: new Uint8Array() });
    call({ op: "wait_finish", operation: eof });
  },
);

test(
  "FIFO stdin lines preserve CRLF and the final unterminated line",
  {
    skip: process.platform === "win32",
    timeout: 5000,
  },
  async (context) => {
    const { call, fd, write, end } = fifoInput(context);
    write(Buffer.from("first\r\nlast"));
    const line = call({ op: "wait_start", fd, kind: "line" }).ok;
    assert.deepEqual(await inputResult(call, line), { ok: "first" });
    call({ op: "wait_finish", operation: line });
    const last = call({ op: "wait_start", fd, kind: "line" }).ok;
    assert.deepEqual(call({ op: "wait_poll", operation: last }), { pending: true });
    end();
    assert.deepEqual(await inputResult(call, last), { ok: "last" });
    call({ op: "wait_finish", operation: last });
    const eof = call({ op: "wait_start", fd, kind: "line" }).ok;
    assert.deepEqual(await inputResult(call, eof), { ok: null });
    call({ op: "wait_finish", operation: eof });
  },
);

test(
  "FIFO stdin close reaps a helper with a pending blocking read",
  {
    skip: process.platform === "win32",
    timeout: 5000,
  },
  async (context) => {
    const { call, fd, write } = fifoInput(context, { manualClose: true });
    write(new Uint8Array([1]));
    const first = call({ op: "wait_start", fd, kind: "bytes", max: 1 }).ok;
    assert.deepEqual(await inputResult(call, first), { ok: new Uint8Array([1]) });
    call({ op: "wait_finish", operation: first });
    const pending = call({ op: "wait_start", fd, kind: "bytes", max: 1 }).ok;
    assert.deepEqual(call({ op: "wait_poll", operation: pending }), { pending: true });
    assert.deepEqual(call({ op: "close" }), { ok: null });
  },
);

test(
  "stdin worker preserves the OS error for a directory descriptor",
  {
    skip: process.platform === "win32",
    timeout: 5000,
  },
  async (context) => {
    const root = fixture(context, {});
    const fd = openSync(root, "r");
    const call = netWorker(context, {
      url: new URL("./input.ts", import.meta.url),
      close: "close",
    });
    context.after(() => closeSync(fd));
    const operation = call({ op: "wait_start", fd, kind: "all" }).ok;
    assert.deepEqual(await inputResult(call, operation), { error: -osConstants.errno.EISDIR });
    call({ op: "wait_finish", operation });
  },
);

test(
  "anonymous pipe stdin still reaches EOF through the readiness stream",
  {
    skip: process.platform === "win32",
    timeout: 6000,
  },
  (context) => {
    const script = String.raw`
    import assert from "node:assert/strict";
    import { setTimeout as delay } from "node:timers/promises";
    import { syncClient } from ${JSON.stringify(new URL("./node.ts", import.meta.url).href)};
    const client = syncClient(new URL(${JSON.stringify(new URL("./input.ts", import.meta.url).href)}), "close");
    const deadline = performance.now() + 2000;
    const call = request => client.call(request, () => deadline - performance.now());
    try {
      const operation = call({ op: "wait_start", fd: 0, kind: "all" }).ok;
      let result;
      do {
        assert.ok(performance.now() < deadline, "pipe EOF deadline");
        result = call({ op: "wait_poll", operation });
        if (result.pending) await delay(5);
      } while (result.pending);
      assert.equal(result.ok, "pipe\r\n");
      call({ op: "wait_finish", operation });
      process.stdout.write(result.ok);
    } finally { client.stop(); }
  `;
    const root = fixture(context, { "pipe-reader.mjs": script });
    // cost: one bounded shell/Node pipeline for an anonymous OS-pipe control.
    const output = execFileSync(
      "/bin/sh",
      [
        "-c",
        'printf "pipe\\r\\n" | "$1" "$2"',
        "stdin-pipe",
        process.execPath,
        join(root, "pipe-reader.mjs"),
      ],
      { timeout: 5000 },
    );
    assert.equal(output.toString(), "pipe\r\n");
  },
);
