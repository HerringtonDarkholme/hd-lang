import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { fileURLToPath } from "node:url";

import { byteOrder, fileOperations, resolvePath, temporaryRoot } from "./node-files.ts";
import { fixture, netHost, positivePid, signalProcess } from "./node-test-support.ts";
import { Err, Ok } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// #815: the peer is not started until the polling thread ran its clocks.
const fifoTest = { skip: process.platform === "win32", timeout: 15000 };
function makeFifo(testContext: TestContext, name = "pipe"): string {
  const root = fixture(testContext, {});
  const path = join(root, name);
  execFileSync("mkfifo", [path]);
  return path;
}
function fifoPeer(
  testContext: TestContext,
  path: string,
  bytes: string | Uint8Array | number[] | null = null,
): Promise<Buffer> {
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
  testContext.after(() => child.kill("SIGKILL"));
  const output: Buffer[] = [],
    errors: Buffer[] = [];
  child.stdout.on("data", (bytes: Buffer) => output.push(bytes));
  child.stderr.on("data", (bytes: Buffer) => errors.push(bytes));
  const done = once(child, "close").then(([code]) => {
    assert.equal(code, 0, Buffer.concat(errors).toString());
    return Buffer.concat(output);
  });
  return done;
}
function siblingClocks(api: Dynamic, operations: number[]): void {
  for (let i = 0; i < 3; i++) {
    const [, clock] = api.host.imports["hd:Clock"]["sleep.start"](0n);
    assert.equal(api.host.imports["hd:rt"].block(), 1);
    api.host.imports["hd:Clock"]["sleep.finish"](clock);
    for (const operation of operations) {
      assert.equal(api.host.core.st.ops.get(operation).completed, false);
    }
  }
}
function finishFile(api: Dynamic, name: string, pending: Dynamic): Dynamic {
  while (!api.host.core.st.ops.get(pending.operation).completed) {
    api.host.imports["hd:rt"].block();
  }
  return api.take(name, pending);
}

test("Fs regular files and path/grant errors stay inline", (testContext) => {
  const root = fixture(testContext, { regular: "plain", "not-dir": "file" });
  const api = netHost(testContext, "hd:FsRead");
  assert.deepEqual(api.call("read_text", join(root, "regular")), { result: Ok("plain") });
  for (const [path, tag] of [
    [join(root, "absent"), "NotFound"],
    [join(root, "not-dir/child"), "NotADirectory"],
    [root, "IsADirectory"],
  ] as [string, string][]) {
    assert.deepEqual(api.call("read_text", path), { result: Err({ tag, path }) });
  }
  const denied = netHost(testContext, "hd:FsRead", { grants: { FsRead: false } });
  const path = join(root, "absent");
  assert.deepEqual(denied.call("read_text", path), {
    result: Err({ tag: "NotGranted", path }),
  });
});

test(
  "Fs FIFO refusals and invalid path prefixes return before offloading",
  fifoTest,
  (testContext) => {
    const path = makeFifo(testContext);
    const denied = netHost(testContext, "hd:FsRead", { grants: { FsRead: false } });
    assert.deepEqual(denied.call("read_text", path), {
      result: Err({ tag: "NotGranted", path }),
    });
    const api = netHost(testContext, "hd:FsRead");
    const invalid = `${path}/../pipe`;
    assert.deepEqual(api.call("read_bytes", invalid), {
      result: Err({ tag: "NotADirectory", path: invalid }),
    });
    const copy = netHost(testContext, "hd:FsWrite", { grants: { FsRead: false, FsWrite: true } });
    assert.deepEqual(copy.call("copy", path, `${path}-copy`), {
      result: Err({ tag: "NotGranted", path }),
    });
  },
);

for (const name of ["read_bytes", "read_text"]) {
  test(
    `Fs ${name} waits for a FIFO while sibling clocks advance`,
    fifoTest,
    async (testContext) => {
      const path = makeFifo(testContext);
      const api = netHost(testContext, "hd:FsRead");
      const pending = api.call(name, path);
      assert.ok(pending.operation);
      siblingClocks(api, [pending.operation]);
      const bytes = Buffer.from("hé\0llo\n");
      const peer = fifoPeer(testContext, path, bytes);
      const result = finishFile(api, name, pending);
      assert.deepEqual(result, Ok(name === "read_text" ? bytes.toString() : new Uint8Array(bytes)));
      await peer;
    },
  );
}

test(
  "Fs FIFO symlinks go pending and invalid UTF-8 keeps the guest path",
  fifoTest,
  async (testContext) => {
    const path = makeFifo(testContext);
    const link = `${path}-link`;
    symlinkSync(path, link);
    const api = netHost(testContext, "hd:FsRead");
    const pending = api.call("read_text", link);
    assert.ok(pending.operation);
    siblingClocks(api, [pending.operation]);
    const peer = fifoPeer(testContext, path, [0xff, 0xfe]);
    assert.deepEqual(
      finishFile(api, "read_text", pending),
      Err({ tag: "InvalidUtf8", path: link }),
    );
    await peer;
  },
);

for (const name of ["write_bytes", "write_text", "append_text"]) {
  test(
    `Fs ${name} waits for a FIFO reader while sibling clocks advance`,
    fifoTest,
    async (testContext) => {
      const path = makeFifo(testContext);
      const api = netHost(testContext, "hd:FsWrite");
      const bytes = Buffer.from("hé\0llo\n");
      const pending = api.call(name, path, name === "write_bytes" ? bytes : bytes.toString());
      assert.ok(pending.operation);
      siblingClocks(api, [pending.operation]);
      const peer = fifoPeer(testContext, path);
      assert.deepEqual(finishFile(api, name, pending), Ok(undefined));
      assert.deepEqual(await peer, bytes);
    },
  );
}

test(
  "Fs independent FIFOs do not exhaust a shared blocking pool",
  fifoTest,
  async (testContext) => {
    const api = netHost(testContext, "hd:FsRead");
    const paths = Array.from({ length: 6 }, (_, i) => makeFifo(testContext, `pipe-${i}`));
    const pending = paths.map((path) => api.call("read_bytes", path));
    siblingClocks(
      api,
      pending.map((item) => item.operation),
    );
    const peers = paths.map((path, i) => fifoPeer(testContext, path, [i, 255]));
    pending.forEach((item, i) => {
      assert.deepEqual(finishFile(api, "read_bytes", item), Ok(new Uint8Array([i, 255])));
    });
    await Promise.all(peers);
  },
);

test(
  "Fs cancelled FIFO reads and copy do not block later calls or close",
  fifoTest,
  async (testContext) => {
    const path = makeFifo(testContext);
    const api = netHost(testContext, "hd:FsRead");
    const pending = api.call("read_text", path);
    siblingClocks(api, [pending.operation]);
    api.host.imports["hd:rt"].abort(pending.operation);
    assert.equal(api.host.core.st.ops.has(pending.operation), false);
    assert.throws(() => api.host.imports["hd:FsRead"]["read_text.finish"](pending.operation));
    const write = netHost(testContext, "hd:FsWrite");
    const copy = write.call("copy", path, `${path}-copy`);
    assert.ok(copy.operation);
    siblingClocks(write, [copy.operation]);
    write.host.imports["hd:rt"].abort(copy.operation);
    const next = makeFifo(testContext, "next");
    const started = api.call("read_text", next);
    const peer = fifoPeer(testContext, next, "successor");
    assert.deepEqual(finishFile(api, "read_text", started), Ok("successor"));
    await peer;
  },
);

test("Fs copy also offloads a special destination", fifoTest, (testContext) => {
  const path = makeFifo(testContext);
  const from = `${path}-source`;
  writeFileSync(from, "copied");
  const api = netHost(testContext, "hd:FsWrite");
  const pending = api.call("copy", from, path);
  assert.ok(pending.operation);
  api.host.imports["hd:rt"].abort(pending.operation);
});

test("Fs metadata and namespace calls on FIFOs stay inline", fifoTest, (testContext) => {
  const path = makeFifo(testContext);
  const api = netHost(testContext, "hd:FsRead");
  assert.equal(api.call("stat", path).result.ok, true);
  const write = netHost(testContext, "hd:FsWrite");
  const moved = `${path}-moved`;
  assert.deepEqual(write.call("rename", path, moved), { result: Ok(undefined) });
  assert.deepEqual(write.call("remove", moved), { result: Ok(undefined) });
});

// A writer that opens the FIFO at once and, on each `send`, writes the
// bytes and answers "wrote", or the error code of a write that failed;
// `close` closes its end.
function fifoWriter(testContext: TestContext, path: string) {
  const child = spawn(
    process.execPath,
    [
      "-e",
      `const fs = require('node:fs');
       const fd = fs.openSync(process.argv[1], 'w');
       require('node:readline').createInterface({ input: process.stdin }).on('line', (line) => {
         if (line === 'close') { fs.closeSync(fd); process.exit(0); }
         try { fs.writeSync(fd, line); console.log('wrote'); }
         catch (error) { console.log(error.code); }
       });`,
      path,
    ],
    { stdio: ["pipe", "pipe", "inherit"] },
  );
  testContext.after(() => child.kill("SIGKILL"));
  const answers = child.stdout.setEncoding("utf8")[Symbol.asyncIterator]();
  let buffered = "";
  const answer = async (): Promise<string> => {
    while (!buffered.includes("\n")) {
      const { value, done } = await answers.next();
      assert.ok(!done, "the writer answers");
      buffered += value;
    }
    const at = buffered.indexOf("\n");
    const line = buffered.slice(0, at);
    buffered = buffered.slice(at + 1);
    return line;
  };
  return {
    send: (text: string): Promise<string> => {
      child.stdin.write(`${text}\n`);
      return answer();
    },
    close: () => child.stdin.write("close\n"),
  };
}
function openFifo(testContext: TestContext, api: Dynamic, path: string) {
  const opening = api.call("open", path);
  assert.ok(opening.operation, "the open of a FIFO with no writer is pending");
  siblingClocks(api, [opening.operation]);
  const writer = fifoWriter(testContext, path);
  const opened = finishFile(api, "open", opening);
  assert.equal(opened.ok, true);
  return { handle: opened.value, writer };
}
const dispose = (api: Dynamic, handle: number): void => {
  assert.deepEqual(api.host.imports["hd:FsRead"]["File.dispose.start"](handle), [0, 0]);
};

// #2414: `open!` and each `read!` of a FIFO wait off the program thread,
// so a clock keeps running until the writer comes and until it writes.
test(
  "Fs open and File.read of a FIFO wait while sibling clocks advance",
  fifoTest,
  async (testContext) => {
    const path = makeFifo(testContext);
    const api = netHost(testContext, "hd:FsRead");
    const { handle, writer } = openFifo(testContext, api, path);
    const read = api.call("File.read", handle, 64);
    assert.ok(read.operation, "a read with nothing written is pending");
    siblingClocks(api, [read.operation]);
    assert.equal(await writer.send("first chunk"), "wrote");
    assert.deepEqual(
      finishFile(api, "File.read", read),
      Ok(new Uint8Array(Buffer.from("first chunk"))),
    );
    // A read asks for at most `max` bytes, and the rest stays in the FIFO.
    assert.equal(await writer.send("abcdef"), "wrote");
    const short = finishFile(api, "File.read", api.call("File.read", handle, 4));
    assert.deepEqual(short, Ok(new Uint8Array(Buffer.from("abcd"))));
    const rest = finishFile(api, "File.read", api.call("File.read", handle, 64));
    assert.deepEqual(rest, Ok(new Uint8Array(Buffer.from("ef"))));
    writer.close();
    assert.deepEqual(
      finishFile(api, "File.read", api.call("File.read", handle, 64)),
      Ok(new Uint8Array()),
    );
    dispose(api, handle);
  },
);

// #2421 (std-io.read.pending-closed): a dispose answers the pending read
// `Closed` at once, the descriptor is released, so the writer's next
// write finds no reader, and no byte reaches the program.
test("Fs disposing a FIFO file answers its pending read Closed", fifoTest, async (testContext) => {
  const path = makeFifo(testContext);
  const api = netHost(testContext, "hd:FsRead");
  const { handle, writer } = openFifo(testContext, api, path);
  const read = api.call("File.read", handle, 64);
  assert.ok(read.operation);
  siblingClocks(api, [read.operation]);
  dispose(api, handle);
  assert.deepEqual(finishFile(api, "File.read", read), Err({ tag: "Closed" }));
  assert.deepEqual(api.call("File.read", handle, 64), { result: Err({ tag: "Closed" }) });
  // The helper that held the descriptor is killed; writes go into the
  // FIFO's buffer until its exit closes the last read end.
  const until = performance.now() + 5000;
  let wrote = await writer.send("late");
  while (wrote === "wrote" && performance.now() < until) {
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
    wrote = await writer.send("late");
  }
  assert.equal(wrote, "EPIPE");
});

// A read the worker answered but the program has not taken is `Closed`
// after a dispose, as a socket's is (std-net.close.pending-closed).
test(
  "Fs a FIFO read answered before a dispose is taken as Closed",
  fifoTest,
  async (testContext) => {
    const path = makeFifo(testContext);
    const api = netHost(testContext, "hd:FsRead");
    const { handle, writer } = openFifo(testContext, api, path);
    const read = api.call("File.read", handle, 64);
    assert.equal(await writer.send("arrived"), "wrote");
    while (!api.host.core.st.ops.get(read.operation).completed) {
      api.host.imports["hd:rt"].block();
    }
    dispose(api, handle);
    assert.deepEqual(api.take("File.read", read), Err({ tag: "Closed" }));
  },
);

// An open whose result the program never takes leaves no file: its
// helper is killed, so the writer's end finds no reader.
test("Fs an aborted FIFO open leaves no reader behind", fifoTest, async (testContext) => {
  const path = makeFifo(testContext);
  const api = netHost(testContext, "hd:FsRead");
  const opening = api.call("open", path);
  const writer = fifoWriter(testContext, path);
  while (!api.host.core.st.ops.get(opening.operation).completed) {
    api.host.imports["hd:rt"].block();
  }
  api.host.imports["hd:rt"].abort(opening.operation);
  const until = performance.now() + 5000;
  let wrote = await writer.send("unread");
  while (wrote === "wrote" && performance.now() < until) {
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
    wrote = await writer.send("unread");
  }
  assert.equal(wrote, "EPIPE");
});

test(
  "Fs finished special-file results can be discarded before guest finish",
  fifoTest,
  (testContext) => {
    const api = netHost(testContext, "hd:FsRead");
    const pending = api.call("read_bytes", "/dev/null");
    assert.ok(pending.operation);
    while (!api.host.core.st.ops.get(pending.operation).completed) {
      api.host.imports["hd:rt"].block();
    }
    api.host.imports["hd:rt"].abort(pending.operation);
    assert.throws(() => api.host.imports["hd:FsRead"]["read_bytes.finish"](pending.operation));
    const next = api.call("read_bytes", "/dev/null");
    assert.deepEqual(finishFile(api, "read_bytes", next), Ok(new Uint8Array()));
  },
);

test(
  "Fs blocked helper exits if its control worker is terminated",
  fifoTest,
  async (testContext) => {
    const path = makeFifo(testContext);
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
        if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error("invalid child pid");
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
    testContext.after(() => child.kill("SIGKILL"));
    const bytes: Buffer[] = [],
      errors: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => bytes.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => errors.push(chunk));
    const [code] = await once(child, "close");
    assert.equal(code, 0, Buffer.concat(errors).toString());
    const pids: number[] = JSON.parse(Buffer.concat(bytes).toString());
    pids.forEach(positivePid);
    assert.equal(pids.length, 1, "one blocked file helper was started");
    // The old helper orphaned its synchronous FIFO open here. Clean up on
    // failure too, so a regression cannot leave a blocked process behind.
    const alive = () =>
      pids.filter((pid) => {
        try {
          signalProcess(pid, 0);
          return true;
        } catch {
          return false;
        }
      });
    testContext.after(() => {
      for (const pid of alive()) {
        signalProcess(pid, "SIGKILL");
      }
    });
    const until = performance.now() + 3000;
    while (alive().length && performance.now() < until) {
      await new Promise<void>((resolve) => setTimeout(resolve, 10));
    }
    assert.deepEqual(alive(), [], "abrupt worker termination leaves no file helper process");
  },
);

// #1901: a path whose last names do not exist finds its longest existing
// prefix in O(log n) realpath calls, not one per missing name, and still
// resolves to the same path.
test("resolvePath of a missing tail asks the OS a logarithmic number of times", (testContext) => {
  const fs = process.getBuiltinModule("node:fs");
  const native = fs.realpathSync.native;
  let calls = 0;
  fs.realpathSync.native = ((path: string) => {
    calls += 1;
    return native(path);
  }) as typeof native;
  testContext.after(() => {
    fs.realpathSync.native = native;
  });
  const root = fixture(testContext, {});
  mkdirSync(join(root, "a/b"), { recursive: true });
  const counts = [8, 80].map((missing) => {
    const tail = Array.from({ length: missing }, (_, i) => `m${i}`);
    const path = join(root, "a/b", ...tail);
    calls = 0;
    assert.equal(resolvePath(path), path);
    return calls;
  });
  // 6 calls at 8 missing names and 9 at 80; one call per missing name took
  // 10 and 82.
  assert.ok(counts[1]! - counts[0]! <= 4, `realpath calls at 8 and 80 missing names: ${counts}`);
  // A `..` after a missing name removes it, and the rest still resolves
  // (joined by hand: path.join would remove the `..` itself).
  assert.equal(resolvePath(`${root}/a/missing/../b`), join(root, "a/b"));
  assert.equal(resolvePath(`${root}/a/missing/../b/new`), join(root, "a/b/new"));
});

// #1901: list_dir sorts its names in UTF-8 byte order without encoding
// them, at n and 10n entries; U+E000 sorts before U+10000, as its bytes do,
// though its UTF-16 code unit is larger than a surrogate's.
test("list_dir orders names by their UTF-8 bytes without allocating per comparison", (testContext) => {
  const from = Buffer.from;
  testContext.after(() => {
    Buffer.from = from;
  });
  for (const count of [10, 100]) {
    const names = Array.from({ length: count }, (_, i) => `f${(i * 7919) % count}`);
    names.push("\uE000", "\u{10000}", "Z", "a", "\u00e9");
    const root = fixture(testContext, Object.fromEntries(names.map((name) => [name, ""])));
    let encoded = 0;
    Buffer.from = ((...args: Parameters<typeof from>) => {
      encoded += 1;
      return from(...args);
    }) as typeof from;
    const listed = (
      fileOperations["hd:FsRead"].list_dir(root) as { value: { path: string }[] }
    ).value.map((item) => item.path.slice(root.length + 1));
    Buffer.from = from;
    assert.equal(encoded, 0, `${count} entries`);
    const bytes = [...names].sort((left, right) => Buffer.compare(from(left), from(right)));
    assert.deepEqual(listed, bytes);
    assert.ok(listed.indexOf("\uE000") < listed.indexOf("\u{10000}"));
  }
  assert.ok(byteOrder("\uFFFF", "\u{10000}") < 0);
  assert.ok(byteOrder("\uD7FF", "\uE000") < 0);
  assert.equal(byteOrder("ab", "ab"), 0);
  assert.ok(byteOrder("ab", "abc") < 0);
});

// #1901: a read the provider has stat'ed takes the regular file's size and
// opens, reads and closes with no second stat; it keeps readFileSync's
// results when the file changed size after that stat, and reads to the
// end when the stat said 0 bytes (a Linux /proc file).
test("read_bytes and read_text read a regular file by the size its stat gave", (testContext) => {
  const root = fixture(testContext, { six: "abcdef", empty: "" });
  const six = join(root, "six");
  const read = fileOperations["hd:FsRead"];
  const bytes = (size: number) =>
    Buffer.from((read.read_bytes(six, size) as { value: Uint8Array }).value).toString();
  assert.deepEqual(read.read_text(six, 6), Ok("abcdef"));
  assert.equal(bytes(6), "abcdef");
  assert.equal(bytes(9), "abcdef", "a file that shrank since its stat");
  assert.equal(bytes(4), "abcd", "a file that grew since its stat, as readFileSync reads it");
  assert.equal(bytes(0), "abcdef", "a stat of 0 bytes reads to the end");
  assert.deepEqual(read.read_text(join(root, "empty"), 0), Ok(""));
  // Through the provider, a regular file, an empty one and a directory.
  const api = netHost(testContext, "hd:FsRead");
  assert.deepEqual(api.call("read_text", six), { result: Ok("abcdef") });
  assert.deepEqual(api.call("read_text", join(root, "empty")), { result: Ok("") });
  assert.deepEqual(api.call("read_text", root), {
    result: Err({ tag: "IsADirectory", path: root }),
  });
});

test("Fs temporary directories are fresh, scoped, and removed recursively", (testContext) => {
  const api = netHost(testContext, "hd:FsWrite", { grants: { FsWrite: ["temp"] } });
  const first = api.call("create_temp_dir", "hd-owned-").result;
  const second = api.call("create_temp_dir", "hd-owned-").result;
  assert.equal(first.ok, true);
  assert.equal(second.ok, true);
  assert.notEqual(first.value, second.value);
  assert.equal(first.value.startsWith(temporaryRoot()), true);
  mkdirSync(join(first.value, "nested"));
  writeFileSync(join(first.value, "nested/file"), "data");
  assert.deepEqual(api.call("remove_dir_all", first.value), { result: Ok(undefined) });
  assert.equal(existsSync(first.value), false);
  assert.equal(existsSync(second.value), true);
  assert.deepEqual(api.call("remove_dir_all", first.value), { result: Ok(undefined) });
  api.host.close();
  assert.equal(existsSync(second.value), false, "unclaimed paths retain a host fallback owner");
});

test("Fs temporary-directory creation checks the OS-temp grant and prefix", (testContext) => {
  const root = fixture(testContext, {});
  const denied = netHost(testContext, "hd:FsWrite", { grants: { FsWrite: [root] } });
  assert.deepEqual(denied.call("create_temp_dir", "refused-"), {
    result: Err({ tag: "NotGranted", path: temporaryRoot() }),
  });
  const granted = netHost(testContext, "hd:FsWrite", { grants: { FsWrite: ["temp"] } });
  for (const prefix of [
    "../escape",
    "a/b",
    "a\0b",
    ...(process.platform === "win32" ? ["a\\b"] : []),
  ]) {
    assert.deepEqual(granted.call("create_temp_dir", prefix), {
      result: Err({
        tag: "Other",
        message: "temporary-directory prefix contains a path separator or NUL",
      }),
    });
  }
  for (const prefix of ["", ".", "..", ...(process.platform === "win32" ? [] : ["literal*?"])]) {
    const made = granted.call("create_temp_dir", prefix).result;
    assert.equal(made.ok, true);
    assert.equal(dirname(made.value), temporaryRoot());
    assert.deepEqual(granted.call("remove_dir_all", made.value), { result: Ok(undefined) });
  }
  const file = join(root, "file");
  writeFileSync(file, "preserve");
  const unrestricted = netHost(testContext, "hd:FsWrite");
  assert.deepEqual(unrestricted.call("remove_dir_all", file), {
    result: Err({ tag: "NotADirectory", path: file }),
  });
  assert.equal(existsSync(file), true);
});
