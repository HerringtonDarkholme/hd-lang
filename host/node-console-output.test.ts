import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { MessageChannel } from "node:worker_threads";

import { createHost } from "./node.ts";
import { fixture, netWorker } from "./node-test-support.ts";
import { createOutput } from "./output.ts";
import { Ok } from "./wire.ts";
import { receiveReply } from "./worker-control.ts";

// #813: the worker reserves original bytes until guest finish.
function inputWorker(t, content) {
  const root = mkdtempSync(join(tmpdir(), "hd-input-"));
  writeFileSync(join(root, "input"), content);
  const fd = openSync(join(root, "input"), "r");
  const call = netWorker(t, { url: new URL("./input.ts", import.meta.url) });
  t.after(() => {
    closeSync(fd);
    rmSync(root, { recursive: true, force: true });
  });
  const start = (kind = "line") => call({ op: "wait_start", kind, fd }).ok;
  const ready = async (operation) => {
    let result;
    await t.waitFor(
      () => {
        result = call({ op: "wait_poll", operation });
        assert.notEqual(result.pending, true);
      },
      { timeout: 5000 },
    );
    return result;
  };
  const read = async (kind = "line") => {
    const operation = start(kind),
      result = await ready(operation);
    call({ op: "wait_finish", operation });
    return result;
  };
  return { call, start, ready, read };
}

test("Console input preserves UTF-8, CRLF and the final unterminated line", async (t) => {
  const input = inputWorker(
    t,
    Buffer.concat([Buffer.from("A\r\né\nbad"), Buffer.from([255]), Buffer.from("\r\nlast\r")]),
  );
  assert.deepEqual(await input.read(), { ok: "A" });
  assert.deepEqual(await input.read("all"), { ok: "é\nbad�\nlast\r" });
  assert.deepEqual(await input.read(), { ok: null });
  assert.deepEqual(await input.read("all"), { ok: "" });
});

for (const reverse of [false, true]) {
  test(`Console input restores concurrent reservations (${reverse ? "reverse" : "forward"} abort order)`, async (t) => {
    const input = inputWorker(t, "first\r\nsecond\nthird\n");
    const first = input.start(),
      second = input.start();
    assert.deepEqual(await input.ready(first), { ok: "first" });
    assert.deepEqual(await input.ready(second), { ok: "second" });
    for (const operation of reverse ? [second, first] : [first, second]) {
      input.call({ op: "wait_abort", operation });
    }
    assert.deepEqual(await input.read(), { ok: "first" });
    assert.deepEqual(await input.read("all"), { ok: "second\nthird\n" });
  });
}

test("Console read_all abort restores bytes after readiness", async (t) => {
  const input = inputWorker(t, "one\r\ntwo");
  const operation = input.start("all");
  assert.deepEqual(await input.ready(operation), { ok: "one\ntwo" });
  input.call({ op: "wait_abort", operation });
  input.call({ op: "wait_abort", operation });
  assert.deepEqual(await input.read(), { ok: "one" });
  assert.deepEqual(await input.read(), { ok: "two" });
  assert.deepEqual(await input.read(), { ok: null });
});

test("Console unreadable input reports a numeric host code", async (t) => {
  const call = netWorker(t, { url: new URL("./input.ts", import.meta.url) });
  const operation = call({ op: "wait_start", kind: "line", fd: -1 }).ok;
  const result = call({ op: "wait_poll", operation });
  assert.equal(typeof result.error, "number");
  call({ op: "wait_finish", operation });
});

// A Console write is one call: it returns its result once the stream has
// taken every byte (module.console.write-sync.never-suspends).
for (const method of ["write_sync", "write_error_sync"]) {
  test(`Console ${method} returns once every byte is written`, async (t) => {
    const root = fixture(t, { output: "" });
    const fd = openSync(join(root, "output"), "w");
    t.after(() => closeSync(fd));
    const output = createOutput();
    t.after(() => output.close());
    const host = createHost({ out() {}, err() {}, write: (_, text) => output.write(fd, text) });
    t.after(() => host.close());
    const memory = new WebAssembly.Memory({ initial: 1 });
    host.prime({ exports: { "hd.x": memory } });
    new Uint8Array(memory.buffer).set(new TextEncoder().encode("written-at-once"));
    assert.equal(typeof host.imports["hd:Console"][method](15), "number");
    assert.equal(readFileSync(join(root, "output"), "utf8"), "written-at-once");
    await output.close();
  });
}

// The queue keeps preceding writes in order, and control requests stay
// live while a chunk waits on a full socket.
test("Output full socket keeps control live", async (t) => {
  const server = net.createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => server.close());
  const accepted = once(server, "connection");
  const writer = net.connect(server.address().port, "127.0.0.1");
  await once(writer, "connect");
  const [reader] = await accepted;
  reader.pause();
  t.after(() => {
    writer.destroy();
    reader.destroy();
  });
  const call = netWorker(t, {
    url: new URL("./output-worker.ts", import.meta.url),
    data: { output: true },
  });
  const { port1, port2 } = new MessageChannel();
  const flag = new Int32Array(new SharedArrayBuffer(4));
  call({ op: "attach", port: port2, flag }, [port2]);
  t.after(() => port1.close());
  const fd = writer._handle.fd;
  const before = "earlier-output\n";
  call({ op: "enqueue", fd, text: before });
  const text = "abcdefg".repeat(2400000);
  const id = 1;
  Atomics.store(flag, 0, 0);
  port1.postMessage({ id, request: { op: "wait_start", fd, text } });
  await t.waitFor(() => assert.deepEqual(call({ op: "idle" }), { ok: false }));
  for (let i = 0; i < 5; i++) {
    assert.deepEqual(call({ op: "idle" }), { ok: false });
  }
  const suffix = '\nerr\t"panic after output"\nran\tcomplete\n';
  call({ op: "enqueue", fd, text: suffix });
  const chunks = [];
  reader.on("data", (bytes) => chunks.push(bytes));
  const ended = once(reader, "end");
  reader.resume();
  await t.waitFor(() => assert.deepEqual(call({ op: "idle" }), { ok: true }), { timeout: 10000 });
  writer.end();
  await ended;
  const received = Buffer.concat(chunks).toString();
  assert(received.startsWith(before));
  assert(received.endsWith(suffix));
  const body = received.slice(before.length, -suffix.length);
  assert.equal(body, text);
  const deadline = performance.now() + 5000;
  const reply = receiveReply(port1, flag, id, () => deadline - performance.now());
  assert.deepEqual(reply?.result, Ok(undefined));
});

test(
  "Output pinned writes finish 2000 calls without per-write sleeps",
  { timeout: 5000 },
  async (t) => {
    const reader = spawn(
      process.execPath,
      [
        "-e",
        'let n = 0; process.stdin.on("data", b => n += b.length); process.stdin.on("end", () => process.stdout.write(String(n)))',
      ],
      { stdio: ["pipe", "pipe", "inherit"] },
    );
    const closed = once(reader, "close");
    t.after(() => {
      if (reader.exitCode === null && reader.signalCode === null) {
        reader.kill("SIGKILL");
      }
    });
    reader.stdout.setEncoding("utf8");
    let received = "";
    reader.stdout.on("data", (text) => (received += text));
    const output = createOutput();
    const pinned = output.connection();
    t.after(() => pinned.port.close());
    const started = performance.now();
    for (let i = 0; i < 2000; i++) {
      assert.deepEqual(output.write(reader.stdin._handle.fd, "x"), Ok(undefined));
    }
    const elapsed = performance.now() - started;
    assert.ok(elapsed < 2000, `2000 pinned writes took ${elapsed.toFixed(0)} ms`);
    await output.close();
    reader.stdin.end();
    const [code, signal] = await closed;
    assert.equal(code, 0);
    assert.equal(signal, null);
    assert.equal(received, "2000");
  },
);

// A write larger than the pipe buffer while the reader is paused: direct
// writes come first, the full pipe starts the writer thread with the rest,
// the write blocks until the reader drains it, later output stays behind
// it, and direct writes resume once the thread's queue is empty.
test(
  "Output to a held stdout pipe blocks, hands off to the thread, and keeps order",
  { skip: process.platform === "win32", timeout: 15000 },
  async (t) => {
    const url = new URL("./output.ts", import.meta.url).href;
    const script = `
      import { writeSync } from "node:fs";
      import { createOutput } from ${JSON.stringify(url)};
      const log = (line) => writeSync(2, line + "\\n");
      const check = (ok, message) => { if (!ok) { log("FAIL " + message); process.exit(1); } };
      const output = createOutput();
      output.out("before\\n");
      check(output.write(1, "small\\n").ok === true, "a direct write answers at once");
      check(!output.threaded(), "no thread before the pipe is full");
      log("blocking");
      check(output.write(1, "x".repeat(4 << 20)).ok === true, "the big write completes");
      check(output.threaded(), "a full pipe handed the rest to the thread");
      output.out("after-full\\n");
      output.err("err-after-full\\n");
      await output.drain();
      output.out("end\\n");
      check(!output.threaded(), "direct writes resume after the thread drains");
      await output.close();
      log("done");
    `;
    const child = spawn(process.execPath, ["--input-type=module", "-e", script], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    t.after(() => child.kill());
    child.stdout.pause();
    let errors = "";
    const closed = once(child, "close");
    await new Promise((resolve, reject) => {
      child.stderr.on("data", (bytes) => {
        errors += bytes;
        if (errors.includes("blocking\n")) {
          setTimeout(resolve, 200);
        }
      });
      closed.then(() => reject(new Error(`exited before the reader resumed: ${errors}`)));
    });
    const chunks = [];
    child.stdout.on("data", (bytes) => chunks.push(bytes));
    child.stdout.resume();
    const [code] = await closed;
    assert.equal(code, 0, errors);
    assert.doesNotMatch(errors, /FAIL/);
    assert.match(errors, /err-after-full\n(.|\n)*done\n$/);
    const text = Buffer.concat(chunks).toString();
    assert.equal(text.length, 4 * 2 ** 20 + "before\nsmall\nafter-full\nend\n".length);
    assert.equal(text, `before\nsmall\n${"x".repeat(4 << 20)}after-full\nend\n`);
  },
);
