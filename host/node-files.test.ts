import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { fixture, netHost } from "./node-test-support.ts";
import { Err, Ok } from "./wire.ts";

// #815: the peer is not started until the polling thread ran its clocks.
const fifoTest = { skip: process.platform === "win32", timeout: 15000 };
function makeFifo(t, name = "pipe") {
  const root = fixture(t, {});
  const path = join(root, name);
  execFileSync("mkfifo", [path]);
  return path;
}
function fifoPeer(t, path, bytes = null) {
  const child = spawn(
    process.execPath,
    [
      "-e",
      `const fs = require('node:fs');
       if (process.argv[2] === 'read') process.stdout.write(fs.readFileSync(process.argv[1]));
       else fs.writeFileSync(process.argv[1], Buffer.from(process.argv[2], 'base64'));`,
      path,
      bytes === null ? "read" : Buffer.from(bytes).toString("base64"),
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  t.after(() => child.kill("SIGKILL"));
  const output = [],
    errors = [];
  child.stdout.on("data", (bytes) => output.push(bytes));
  child.stderr.on("data", (bytes) => errors.push(bytes));
  const done = once(child, "close").then(([code]) => {
    assert.equal(code, 0, Buffer.concat(errors).toString());
    return Buffer.concat(output);
  });
  return done;
}
function siblingClocks(api, operations) {
  for (let i = 0; i < 3; i++) {
    const [, clock] = api.host.imports["hd:Clock"]["sleep.start"](0n);
    assert.equal(api.host.imports["hd:rt"].block(), 1);
    api.host.imports["hd:Clock"]["sleep.finish"](clock);
    for (const operation of operations) {
      assert.equal(api.host.core.st.ops.get(operation).completed, false);
    }
  }
}
function finishFile(api, name, pending) {
  while (!api.host.core.st.ops.get(pending.operation).completed) {
    api.host.imports["hd:rt"].block();
  }
  return api.take(name, pending);
}

test("Fs regular files and path/grant errors stay inline", (t) => {
  const root = fixture(t, { regular: "plain", "not-dir": "file" });
  const api = netHost(t, "hd:FsRead");
  assert.deepEqual(api.call("read_text", join(root, "regular")), { result: Ok("plain") });
  for (const [path, tag] of [
    [join(root, "absent"), "NotFound"],
    [join(root, "not-dir/child"), "NotADirectory"],
    [root, "IsADirectory"],
  ]) {
    assert.deepEqual(api.call("read_text", path), { result: Err({ tag, path }) });
  }
  const denied = netHost(t, "hd:FsRead", { grants: { FsRead: false } });
  const path = join(root, "absent");
  assert.deepEqual(denied.call("read_text", path), {
    result: Err({ tag: "NotGranted", path }),
  });
});

test("Fs FIFO refusals and invalid path prefixes return before offloading", fifoTest, (t) => {
  const path = makeFifo(t);
  const denied = netHost(t, "hd:FsRead", { grants: { FsRead: false } });
  assert.deepEqual(denied.call("read_text", path), {
    result: Err({ tag: "NotGranted", path }),
  });
  const api = netHost(t, "hd:FsRead");
  const invalid = `${path}/../pipe`;
  assert.deepEqual(api.call("read_bytes", invalid), {
    result: Err({ tag: "NotADirectory", path: invalid }),
  });
  const copy = netHost(t, "hd:FsWrite", { grants: { FsRead: false, FsWrite: true } });
  assert.deepEqual(copy.call("copy", path, `${path}-copy`), {
    result: Err({ tag: "NotGranted", path }),
  });
});

for (const name of ["read_bytes", "read_text"]) {
  test(`Fs ${name} waits for a FIFO while sibling clocks advance`, fifoTest, async (t) => {
    const path = makeFifo(t);
    const api = netHost(t, "hd:FsRead");
    const pending = api.call(name, path);
    assert.ok(pending.operation);
    siblingClocks(api, [pending.operation]);
    const bytes = Buffer.from("hé\0llo\n");
    const peer = fifoPeer(t, path, bytes);
    const result = finishFile(api, name, pending);
    assert.deepEqual(result, Ok(name === "read_text" ? bytes.toString() : new Uint8Array(bytes)));
    await peer;
  });
}

test("Fs FIFO symlinks go pending and invalid UTF-8 keeps the guest path", fifoTest, async (t) => {
  const path = makeFifo(t);
  const link = `${path}-link`;
  symlinkSync(path, link);
  const api = netHost(t, "hd:FsRead");
  const pending = api.call("read_text", link);
  assert.ok(pending.operation);
  siblingClocks(api, [pending.operation]);
  const peer = fifoPeer(t, path, [0xff, 0xfe]);
  assert.deepEqual(finishFile(api, "read_text", pending), Err({ tag: "InvalidUtf8", path: link }));
  await peer;
});

for (const name of ["write_bytes", "write_text", "append_text"]) {
  test(`Fs ${name} waits for a FIFO reader while sibling clocks advance`, fifoTest, async (t) => {
    const path = makeFifo(t);
    const api = netHost(t, "hd:FsWrite");
    const bytes = Buffer.from("hé\0llo\n");
    const pending = api.call(name, path, name === "write_bytes" ? bytes : bytes.toString());
    assert.ok(pending.operation);
    siblingClocks(api, [pending.operation]);
    const peer = fifoPeer(t, path);
    assert.deepEqual(finishFile(api, name, pending), Ok(undefined));
    assert.deepEqual(await peer, bytes);
  });
}

test("Fs independent FIFOs do not exhaust a shared blocking pool", fifoTest, async (t) => {
  const api = netHost(t, "hd:FsRead");
  const paths = Array.from({ length: 6 }, (_, i) => makeFifo(t, `pipe-${i}`));
  const pending = paths.map((path) => api.call("read_bytes", path));
  siblingClocks(
    api,
    pending.map((p) => p.operation),
  );
  const peers = paths.map((path, i) => fifoPeer(t, path, [i, 255]));
  pending.forEach((p, i) => {
    assert.deepEqual(finishFile(api, "read_bytes", p), Ok(new Uint8Array([i, 255])));
  });
  await Promise.all(peers);
});

test("Fs cancelled FIFO reads and copy do not block later calls or close", fifoTest, async (t) => {
  const path = makeFifo(t);
  const api = netHost(t, "hd:FsRead");
  const pending = api.call("read_text", path);
  siblingClocks(api, [pending.operation]);
  api.host.imports["hd:rt"].abort(pending.operation);
  assert.equal(api.host.core.st.ops.has(pending.operation), false);
  assert.throws(() => api.host.imports["hd:FsRead"]["read_text.finish"](pending.operation));
  const write = netHost(t, "hd:FsWrite");
  const copy = write.call("copy", path, `${path}-copy`);
  assert.ok(copy.operation);
  siblingClocks(write, [copy.operation]);
  write.host.imports["hd:rt"].abort(copy.operation);
  const next = makeFifo(t, "next");
  const started = api.call("read_text", next);
  const peer = fifoPeer(t, next, "successor");
  assert.deepEqual(finishFile(api, "read_text", started), Ok("successor"));
  await peer;
});

test("Fs copy also offloads a special destination", fifoTest, (t) => {
  const path = makeFifo(t);
  const from = `${path}-source`;
  writeFileSync(from, "copied");
  const api = netHost(t, "hd:FsWrite");
  const pending = api.call("copy", from, path);
  assert.ok(pending.operation);
  api.host.imports["hd:rt"].abort(pending.operation);
});

test("Fs metadata and namespace calls on FIFOs stay inline", fifoTest, (t) => {
  const path = makeFifo(t);
  const api = netHost(t, "hd:FsRead");
  assert.equal(api.call("stat", path).result.ok, true);
  const write = netHost(t, "hd:FsWrite");
  const moved = `${path}-moved`;
  assert.deepEqual(write.call("rename", path, moved), { result: Ok(undefined) });
  assert.deepEqual(write.call("remove", moved), { result: Ok(undefined) });
});

test("Fs finished special-file results can be discarded before guest finish", fifoTest, (t) => {
  const api = netHost(t, "hd:FsRead");
  const pending = api.call("read_bytes", "/dev/null");
  assert.ok(pending.operation);
  while (!api.host.core.st.ops.get(pending.operation).completed) {
    api.host.imports["hd:rt"].block();
  }
  api.host.imports["hd:rt"].abort(pending.operation);
  assert.throws(() => api.host.imports["hd:FsRead"]["read_bytes.finish"](pending.operation));
  const next = api.call("read_bytes", "/dev/null");
  assert.deepEqual(finishFile(api, "read_bytes", next), Ok(new Uint8Array()));
});

test("Fs blocked helper exits if its control worker is terminated", fifoTest, async (t) => {
  const path = makeFifo(t);
  const source = `
    const { execFileSync } = require('node:child_process');
    const { Worker, MessageChannel, receiveMessageOnPort } = require('node:worker_threads');
    const { port1, port2 } = new MessageChannel();
    const flag = new Int32Array(new SharedArrayBuffer(4));
    const worker = new Worker(process.argv[1], {
      workerData: { port: port2, flag }, transferList: [port2],
    });
    port1.postMessage({ id: 1, request: {
      op: 'wait_start', module: 'hd:FsRead', name: 'read_text', args: [process.argv[2]],
    } });
    if (Atomics.wait(flag, 0, 0, 5000) === 'timed-out') process.exit(1);
    receiveMessageOnPort(port1);
    setTimeout(async () => {
      const children = execFileSync('ps', ['-Ao', 'pid,ppid'], { encoding: 'utf8' })
        .trim().split('\\n').slice(1).map(row => row.trim().split(/\\s+/).map(Number))
        .filter(([, parent]) => parent === process.pid).map(([pid]) => pid);
      // Exclude the ps command itself: it has already exited now.
      const alive = children.filter(pid => {
        try { process.kill(pid, 0); return true; } catch { return false; }
      });
      console.log(JSON.stringify(alive));
      port1.close();
      await worker.terminate();
    }, 200);
  `;
  const child = spawn(
    process.execPath,
    ["-e", source, fileURLToPath(new URL("./fs.ts", import.meta.url)), path],
    {
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  t.after(() => child.kill("SIGKILL"));
  const bytes = [],
    errors = [];
  child.stdout.on("data", (b) => bytes.push(b));
  child.stderr.on("data", (b) => errors.push(b));
  const [code] = await once(child, "close");
  assert.equal(code, 0, Buffer.concat(errors).toString());
  const pids = JSON.parse(Buffer.concat(bytes).toString());
  assert.equal(pids.length, 1, "one blocked file helper was started");
  // The old helper orphaned its synchronous FIFO open here. Clean up on
  // failure too, so a regression cannot leave a blocked process behind.
  const alive = () =>
    pids.filter((pid) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch {
        return false;
      }
    });
  t.after(() => {
    for (const pid of alive()) {
      process.kill(pid, "SIGKILL");
    }
  });
  const until = performance.now() + 3000;
  while (alive().length && performance.now() < until) {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.deepEqual(alive(), [], "abrupt worker termination leaves no file helper process");
});
