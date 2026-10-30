import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import {
  closeSync,
  openSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import net from "node:net";
import dgram from "node:dgram";
import http from "node:http";
import https from "node:https";
import { once } from "node:events";
import { MessageChannel, Worker } from "node:worker_threads";
import {
  createCoreHost,
  httpProvider,
  httpSettings,
  pendingOperation,
  TimeLimit,
} from "./core.mts";
import { Dec, Enc, Err, HostContract, Ok, str } from "./wire.mts";
import { ConsoleError, METHODS } from "./gen/abi.mts";

import { cgroupCpuCount, createHost, syncClient } from "./node.mjs";
import { createOutput } from "./output.mts";
import { browserSend } from "./web.mts";
import { failed as failedOutput } from "./output-worker.mts";
import { postReply, receiveReply } from "./worker-control.mts";

function fixture(t, files) {
  // The resolved path: grant errors report a path with its links resolved,
  // and the temporary directory may sit behind one (macOS /var).
  const root = realpathSync(mkdtempSync(join(tmpdir(), "hd-cgroup-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

test("Console errors carry only stream tags and numeric host codes", () => {
  assert.deepEqual(failedOutput({ code: "EPIPE", errno: -32 }), Err({ tag: "Closed" }));
  assert.deepEqual(
    failedOutput({ code: "EIO", errno: -5, message: "host-specific text" }),
    Err({ tag: "Other", code: -5 }),
  );
  assert.deepEqual(
    failedOutput({ code: "UNKNOWN", message: "no numeric code" }),
    Err({ tag: "Other", code: 0 }),
  );
  assert.throws(() => ConsoleError.dec(new Dec(Uint8Array.of(1, 3))), HostContract);
  assert.throws(() => ConsoleError.dec(new Dec(Uint8Array.of(2))), HostContract);
});

// Deliberately violate notification timing in a worker, while using the
// production client. Queue publication and the wake hint must be independent.
function controlClient(t) {
  const root = fixture(t, {
    "control.mjs": `
      import {workerData} from 'node:worker_threads';
      const {port,flag}=workerData;
      const notify=()=>{Atomics.store(flag,0,1);Atomics.notify(flag,0);};
      port.on('message',({id,request})=>{
        const send=()=>port.postMessage({id,result:{id,value:request.op}});
        if(request.op==='empty') { notify(); setTimeout(send,30); }
        else if(request.op==='old') {
          port.postMessage({id:id-1,result:{value:'stale'}});
          notify(); setTimeout(send,30);
        } else if(request.op==='late') { notify(); setTimeout(send,150); }
        else if(request.op==='never') { notify(); }
        else { send(); notify(); }
      });
    `,
  });
  const client = syncClient(pathToFileURL(join(root, "control.mjs")));
  t.after(() => client.stop());
  return {
    ...client,
    call: (request, left = null) => {
      const deadline = performance.now() + 5000;
      return client.call(request, left ?? (() => deadline - performance.now()));
    },
  };
}

test("worker control waits for a message after an empty notification", (t) => {
  const client = controlClient(t);
  const deadline = performance.now() + 5000;
  assert.equal(client.call({ op: "empty" }, () => deadline - performance.now()).value, "empty");
});

test("worker control ignores an old call ID before the current reply", (t) => {
  const client = controlClient(t);
  const first = client.call({ op: "first" });
  const next = client.call({ op: "old" });
  assert.equal(next.value, "old");
  assert.ok(next.id > first.id);
});

for (const op of ["late", "never"]) {
  test(`worker control times out and restarts after ${op} replies`, (t) => {
    const client = controlClient(t);
    const first = client.call({ op: "first" });
    const deadline = performance.now() + 30;
    assert.throws(() => client.call({ op }, () => deadline - performance.now()), TimeLimit);
    assert.equal(client.started(), false);
    const current = client.call({ op: "current" });
    assert.equal(current.value, "current");
    assert.ok(current.id > first.id);
  });
}

test("worker control publishes a reply before setting its wake hint", () => {
  const flag = new Int32Array(new SharedArrayBuffer(4));
  let message;
  postReply(
    {
      postMessage: (m) => {
        assert.equal(Atomics.load(flag, 0), 0);
        message = m;
      },
    },
    flag,
    7,
    { ok: 9 },
  );
  assert.deepEqual(message, { id: 7, result: { ok: 9 } });
  assert.equal(Atomics.load(flag, 0), 1);
});

for (const [name, file, request, close, message] of [
  ["HTTP", "http.mjs", { op: "poll", operation: 0 }, null, "unknown HTTP operation"],
  ["Process", "process.mjs", { op: "wait_poll", operation: 0 }, "close", "unknown process wait"],
]) {
  test(`${name} worker protocol faults are host contracts`, (t) => {
    const client = syncClient(new URL(`./${file}`, import.meta.url), close);
    t.after(() => client.stop());
    assert.throws(
      () => client.call(request),
      (error) => error instanceof HostContract && error.message === message,
    );
    assert.equal(client.started(), false);
  });
}

// #408: libuv's last finite ancestor must not override a stricter child.
test("v2: one-CPU child inside a four-CPU parent offers one CPU", (t) => {
  const root = fixture(t, {
    "cpu.max": "max 100000\n",
    "parent/cpu.max": "400000 100000\n",
    "parent/child/cpu.max": "100000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v2: the tightest ancestor can be the parent", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "100000 100000\n",
    "parent/child/cpu.max": "400000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 8), 1);
});

test("v2: an unlimited parent preserves its child's limit", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "max 100000\n",
    "parent/child/cpu.max": "100000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v2: an unlimited child inherits its parent's limit", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "400000 100000\n",
    "parent/child/cpu.max": "max 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 8), 4);
});

test("v1: each quota uses its own period and ancestors take the minimum", (t) => {
  const root = fixture(t, {
    "parent/cpu.cfs_quota_us": "800000\n",
    "parent/cpu.cfs_period_us": "200000\n",
    "parent/child/cpu.cfs_quota_us": "100000\n",
    "parent/child/cpu.cfs_period_us": "100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v1: negative unlimited quota does not erase a finite child", (t) => {
  const root = fixture(t, {
    "parent/cpu.cfs_quota_us": "-1\n",
    "parent/cpu.cfs_period_us": "100000\n",
    "parent/child/cpu.cfs_quota_us": "100000\n",
    "parent/child/cpu.cfs_period_us": "100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("the OS affinity count can be stricter than any quota", (t) => {
  const root = fixture(t, { "cpu.max": "400000 100000\n" });
  assert.equal(cgroupCpuCount(root, "/", 2), 2);
});

test("fractional quotas round down like Rust, with at least one CPU", (t) => {
  for (const quota of [150000, 50000]) {
    const root = fixture(t, { "cpu.max": `${quota} 100000\n` });
    assert.equal(cgroupCpuCount(root, "/", 8), 1);
  }
});

test("missing cgroup filesystem leaves the OS count intact", (t) => {
  const root = fixture(t, {});
  assert.equal(cgroupCpuCount(join(root, "absent"), "/parent/child", 4), 4);
});

test("malformed, zero-period and missing limits do not hide an ancestor", (t) => {
  const root = fixture(t, {
    "cpu.max": "200000 100000\n",
    "parent/cpu.max": "400000 0\n",
    "parent/child/cpu.max": "not-a-quota\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child/missing", 8), 2);
});

test("the walk stops at its cgroup mount", (t) => {
  const root = fixture(t, {
    "cpu.max": "100000 100000\n",
    "mount/cpu.max": "400000 100000\n",
  });
  assert.equal(cgroupCpuCount(join(root, "mount"), "/", 8), 4);
});

// #757: a provider may stay pending while the guest polls its deadline.
function operationHost(ready, onDone, onAbort, onFinish = null) {
  const host = createCoreHost(
    { out() {}, err() {} },
    {},
    {
      providers: () => ({
        traits: {
          "hd:ConsoleInput": {
            read_line: () =>
              pendingOperation(
                ready,
                () => {
                  onDone();
                  return Ok(null);
                },
                onAbort,
                onFinish,
              ),
          },
        },
      }),
    },
  );
  const memory = new WebAssembly.Memory({ initial: 1 });
  host.prime({ exports: { "hd.x": memory } });
  const start = () => host.imports["hd:ConsoleInput"]["read_line.start"]();
  return { host, memory, start };
}

test("pending external I/O does not hide a ready clock", () => {
  let aborted = 0;
  const { host, memory, start } = operationHost(
    () => false,
    () => assert.fail("pending operation completed"),
    () => {
      aborted++;
    },
  );
  const [status, pending] = start();
  assert.equal(status, 1);
  const [clockStatus, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
  assert.equal(clockStatus, 1);
  assert.equal(host.imports["hd:rt"].block(), 1);
  assert.equal(new DataView(memory.buffer).getInt32(0, true), clock);
  host.imports["hd:Clock"]["sleep.finish"](clock);
  host.imports["hd:rt"].abort(pending);
  host.imports["hd:rt"].abort(pending);
  host.close();
  assert.equal(aborted, 1);
});

test("an I/O retry delay cannot hide readiness when another operation wakes", () => {
  let ready = false;
  const { host, start } = operationHost(
    () => ready,
    () => {},
    () => {},
  );
  const [, pending] = start();
  const [, firstClock] = host.imports["hd:Clock"]["sleep.start"](0n);
  assert.equal(host.imports["hd:rt"].block(), 1);
  host.imports["hd:Clock"]["sleep.finish"](firstClock);
  // A retry deadline controls idle waiting, not whether the host probes
  // I/O on another wake. Stretch it to make that distinction deterministic.
  host.core.st.ops.get(pending).at = performance.now() + 60_000;
  ready = true;
  const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
  assert.equal(host.imports["hd:rt"].block(), 2);
  host.imports["hd:ConsoleInput"]["read_line.finish"](pending);
  host.imports["hd:Clock"]["sleep.finish"](clock);
  host.close();
});

test("abort releases an external result completed before finish", () => {
  let done = 0,
    aborted = 0;
  const { host, start } = operationHost(
    () => true,
    () => {
      done++;
    },
    () => {
      aborted++;
    },
  );
  const [status, pending] = start();
  assert.equal(status, 1);
  assert.equal(host.imports["hd:rt"].block(), 1);
  assert.equal(done, 1);
  host.imports["hd:rt"].abort(pending);
  host.close();
  assert.equal(aborted, 1);
});

test("finish transfers an external result without calling abort", () => {
  let aborted = 0;
  const { host, start } = operationHost(
    () => true,
    () => {},
    () => {
      aborted++;
    },
  );
  const [, pending] = start();
  assert.equal(host.imports["hd:rt"].block(), 1);
  host.imports["hd:ConsoleInput"]["read_line.finish"](pending);
  host.close();
  assert.equal(aborted, 0);
});

test("the external finish hook runs only at a validated guest finish", () => {
  let delivered = 0;
  const { host, start } = operationHost(
    () => true,
    () => {},
    () => {},
    () => {
      delivered++;
    },
  );
  const [, operation] = start();
  assert.throws(() => host.imports["hd:ConsoleInput"]["read_line.finish"](operation));
  assert.equal(delivered, 0, "an unfinished result cannot transfer ownership");
  host.imports["hd:rt"].block();
  assert.equal(delivered, 0, "completion is not guest delivery");
  host.imports["hd:ConsoleInput"]["read_line.finish"](operation);
  assert.equal(delivered, 1);
  assert.throws(() => host.imports["hd:ConsoleInput"]["read_line.finish"](operation));
  assert.equal(delivered, 1, "a stale finish cannot deliver twice");
  host.close();
});

test("abort after completion does not run the external finish hook", () => {
  let delivered = 0,
    aborted = 0;
  const { host, start } = operationHost(
    () => true,
    () => {},
    () => {
      aborted++;
    },
    () => {
      delivered++;
    },
  );
  const [, operation] = start();
  host.imports["hd:rt"].block();
  host.imports["hd:rt"].abort(operation);
  assert.equal(delivered, 0);
  assert.equal(aborted, 1);
  host.close();
});

test("host close releases pending and completed untaken operations", () => {
  let aborted = 0;
  const { host, start } = operationHost(
    () => true,
    () => {},
    () => {
      aborted++;
    },
  );
  start();
  assert.equal(host.imports["hd:rt"].block(), 1);
  start();
  host.close();
  host.close();
  assert.equal(aborted, 2);
});

test("the async entry driver polls external readiness before waking the guest", async () => {
  let ready = false,
    done = 0,
    pending;
  const { host, memory, start } = operationHost(
    () => ready,
    () => {
      done++;
    },
    () => {},
  );
  const instance = {
    exports: {
      "hd.x": memory,
      init() {},
      poll() {
        if (pending === undefined) {
          [, pending] = start();
          return -1;
        }
        host.imports["hd:ConsoleInput"]["read_line.finish"](pending);
        return 0;
      },
      "hd.wake": (count) => {
        assert.equal(count, 1);
        assert.equal(ready, true);
        assert.equal(new DataView(memory.buffer).getInt32(0, true), pending);
      },
    },
  };
  const timer = setTimeout(() => {
    ready = true;
  }, 10);
  try {
    assert.equal((await host.run(instance, "init", "poll")).status, 0);
    assert.equal(done, 1);
  } finally {
    clearTimeout(timer);
    host.close();
  }
});

function netWorker(t, options = {}) {
  const { port1, port2 } = new MessageChannel();
  const flag = new Int32Array(new SharedArrayBuffer(4));
  const worker = new Worker(options.url ?? new URL("./net.mjs", import.meta.url), {
    workerData: { port: port2, flag, ...options.data },
    transferList: [port2],
  });
  t.after(async () => {
    if (options.close) {
      call({ op: options.close });
    }
    port1.close();
    await worker.terminate();
  });
  let nextCall = 1;
  const call = (request, transfer = []) => {
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    Atomics.store(flag, 0, 0);
    port1.postMessage({ id, request }, transfer);
    const reply = receiveReply(port1, flag, id, () => deadline - performance.now());
    assert.ok(reply, "worker control reply");
    return reply.result;
  };
  call.worker = worker;
  return call;
}

async function loopback(t) {
  const sockets = new Set();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("error", () => {});
    socket.once("close", () => sockets.delete(socket));
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  t.after(() => {
    for (const socket of sockets) {
      socket.destroy();
    }
    server.close();
  });
  return { host: "127.0.0.1", port: server.address().port, address: "loopback" };
}

// Exercise the real provider through the ABI, including complete-before-finish.
function netHost(t, trait = "hd:Net", env = {}, make = createHost) {
  const host = make({ out() {}, err() {} }, env);
  host.core.st.deadline = performance.now() + 5000;
  t.after(() => host.close());
  const memory = new WebAssembly.Memory({ initial: trait === "hd:Process" ? 32 : 1 });
  host.prime({ exports: { "hd.x": memory } });
  const imports = host.imports[trait];
  const decode = (name, len) =>
    METHODS[trait][name].result.dec(new Dec(new Uint8Array(memory.buffer, 0, len)));
  const call = (name, ...args) => {
    const method = METHODS[trait][name];
    const encoded = new Enc(),
      raw = [];
    if (method.params.length === 1 && method.params[0] === str) {
      const bytes = new TextEncoder().encode(args[0]);
      new Uint8Array(memory.buffer).set(bytes);
      raw.push(bytes.length);
    } else {
      method.params.forEach((param, i) => {
        if (typeof param === "string") {
          raw.push(args[i]);
        } else {
          param.enc(encoded, args[i]);
        }
      });
      if (raw.length !== method.params.length) {
        new Uint8Array(memory.buffer).set(encoded.done());
        raw.push(encoded.len);
      }
    }
    if (method.wait === "never") {
      return { result: decode(name, imports[name](...raw)) };
    }
    const [status, value] = imports[`${name}.start`](...raw);
    return status === 0 ? { result: decode(name, value) } : { operation: value };
  };
  const take = (name, started) => {
    if (started.result) {
      return started.result;
    }
    if (!host.core.st.ops.get(started.operation).completed) {
      host.imports["hd:rt"].block();
    }
    return decode(name, imports[`${name}.finish`](started.operation));
  };
  return { host, call, take };
}

async function tcpHost(t) {
  let connected;
  const arrived = new Promise((resolve) => {
    connected = resolve;
  });
  const server = net.createServer((socket) => connected(socket));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const api = netHost(t);
  const stream = api.take("connect", api.call("connect", "127.0.0.1", server.address().port));
  assert(stream.ok);
  const peer = await arrived;
  t.after(() => peer.destroy());
  return { ...api, handle: stream.value, peer };
}

for (const phase of ["pending", "completed before finish", "completed past deadline"]) {
  test(`TCP abort ${phase} preserves input and byte order`, { timeout: 10000 }, async (t) => {
    const { host, call, take, handle, peer } = await tcpHost(t);
    const first = call("TcpStream.read", handle, 1);
    assert.notEqual(first.operation, undefined);
    if (phase !== "pending") {
      peer.write("abc");
      host.imports["hd:rt"].block();
      assert(host.core.st.ops.get(first.operation).completed);
      assert.throws(() => host.imports["hd:Net"]["TcpListener.accept.finish"](first.operation));
    }
    const deadline = host.core.st.deadline;
    if (phase === "completed past deadline") {
      host.core.st.deadline = performance.now() - 1;
    }
    host.imports["hd:rt"].abort(first.operation);
    host.core.st.deadline = deadline;
    if (phase === "pending") {
      peer.write("abc");
    }
    for (const byte of ["a", "b", "c"]) {
      const result = take("TcpStream.read", call("TcpStream.read", handle, 1));
      assert(result.ok);
      assert.equal(Buffer.from(result.value).toString(), byte);
    }
  });
}

for (const reversed of [false, true]) {
  test(
    `concurrent TCP aborts preserve arrival order (reverse=${reversed})`,
    { timeout: 10000 },
    async (t) => {
      const { host, call, take, handle, peer } = await tcpHost(t);
      const first = call("TcpStream.read", handle, 1);
      const second = call("TcpStream.read", handle, 1);
      peer.write("abc");
      while (
        !host.core.st.ops.get(first.operation).completed ||
        !host.core.st.ops.get(second.operation).completed
      ) {
        host.imports["hd:rt"].block();
      }
      const operations = [first.operation, second.operation];
      if (reversed) {
        operations.reverse();
      }
      for (const operation of operations) {
        host.imports["hd:rt"].abort(operation);
      }
      for (const byte of ["a", "b", "c"]) {
        assert.equal(
          Buffer.from(take("TcpStream.read", call("TcpStream.read", handle, 1)).value).toString(),
          byte,
        );
      }
    },
  );
}

test("closing a stream completes a pending read as Closed", { timeout: 10000 }, async (t) => {
  const { call, take, handle } = await tcpHost(t);
  const read = call("TcpStream.read", handle, 1);
  assert(call("TcpStream.close", handle).result.ok);
  const result = take("TcpStream.read", read);
  assert.equal(result.ok, false);
  assert.equal(result.error.tag, "Closed");
});

test(
  "a completed read closed before guest finish returns Closed",
  { timeout: 10000 },
  async (t) => {
    const { host, call, take, handle, peer } = await tcpHost(t);
    const read = call("TcpStream.read", handle, 1);
    peer.write("a");
    host.imports["hd:rt"].block();
    assert(host.core.st.ops.get(read.operation).completed);
    assert(call("TcpStream.close", handle).result.ok);
    const result = take("TcpStream.read", read);
    assert.equal(result.ok, false);
    assert.equal(result.error.tag, "Closed");
  },
);

test(
  "aborting a completed truncated UDP receive restores the whole datagram",
  { timeout: 10000 },
  async (t) => {
    const { host, call, take } = netHost(t);
    const bound = take("bind_udp", call("bind_udp", "127.0.0.1", 0));
    assert(bound.ok);
    const [, port] = call("UdpSocket.local_address", bound.value).result.value;
    const receive = call("UdpSocket.receive", bound.value, 1);
    assert.notEqual(receive.operation, undefined);
    const peer = dgram.createSocket("udp4");
    t.after(() => peer.close());
    await new Promise((resolve, reject) =>
      peer.send(Buffer.from("abc"), port, "127.0.0.1", (e) => (e ? reject(e) : resolve())),
    );
    host.imports["hd:rt"].block();
    host.imports["hd:rt"].abort(receive.operation);
    const result = take("UdpSocket.receive", call("UdpSocket.receive", bound.value, 3));
    assert(result.ok);
    assert.equal(Buffer.from(result.value.bytes).toString(), "abc");
  },
);

test("aborting a completed accept closes its undelivered stream", { timeout: 10000 }, async (t) => {
  const { host, call, take } = netHost(t);
  const bound = take("listen", call("listen", "127.0.0.1", 0));
  assert(bound.ok);
  const [, port] = call("TcpListener.local_address", bound.value).result.value;
  const accept = call("TcpListener.accept", bound.value);
  assert.notEqual(accept.operation, undefined);
  const peer = net.connect({ host: "127.0.0.1", port });
  t.after(() => peer.destroy());
  await once(peer, "connect");
  host.imports["hd:rt"].block();
  const closed = once(peer, "close");
  host.imports["hd:rt"].abort(accept.operation);
  await closed;
});

test(
  "aborting a pending accept leaves the next connection for its successor",
  { timeout: 10000 },
  async (t) => {
    const { host, call, take } = netHost(t);
    const bound = take("listen", call("listen", "127.0.0.1", 0));
    assert(bound.ok);
    const [, port] = call("TcpListener.local_address", bound.value).result.value;
    const abandoned = call("TcpListener.accept", bound.value);
    assert.notEqual(abandoned.operation, undefined);
    host.imports["hd:rt"].abort(abandoned.operation);
    const next = call("TcpListener.accept", bound.value);
    const peer = net.connect({ host: "127.0.0.1", port });
    t.after(() => peer.destroy());
    await once(peer, "connect");
    const accepted = take("TcpListener.accept", next);
    assert(accepted.ok);
    assert(call("TcpStream.close", accepted.value).result.ok);
  },
);

test("worker abort immediately releases the connect operation", async (t) => {
  const address = await loopback(t),
    call = netWorker(t);
  for (let i = 0; i < 8; i++) {
    const { ok: operation } = call({ op: "connect_start", ...address });
    assert.equal(typeof operation, "number");
    call({ op: "connect_abort", operation });
    assert.equal(call({ op: "open" }).ok.stream, 0);
  }
});

test("worker abort releases a completed stream that was never delivered", async (t) => {
  const address = await loopback(t),
    call = netWorker(t);
  const { ok: operation } = call({ op: "connect_start", ...address });
  await t.waitFor(() => assert.equal(call({ op: "open" }).ok.stream, 1), {
    interval: 1,
    timeout: 5000,
  });
  assert.equal(call({ op: "open" }).ok.stream, 1);
  call({ op: "connect_abort", operation });
  assert.equal(call({ op: "open" }).ok.stream, 0);
});

test("worker result transfers one ready stream and close releases it", async (t) => {
  const address = await loopback(t),
    call = netWorker(t);
  const { ok: operation } = call({ op: "connect_start", ...address });
  await t.waitFor(() => assert.equal(call({ op: "open" }).ok.stream, 1), {
    interval: 1,
    timeout: 5000,
  });
  const result = call({ op: "connect_poll", operation });
  assert.equal(typeof result.ok, "number");
  assert.equal(call({ op: "open" }).ok.stream, 1);
  call({ op: "close", handle: result.ok });
  assert.equal(call({ op: "open" }).ok.stream, 0);
});

// The peer completes its kernel connect while the worker cannot run any
// network callback. Invoke accept in that same callback on resume, so the
// regression cannot pass merely because message dispatch happened later.
test(
  "zero-bound accept sees a kernel-queued peer before its JS callback",
  { timeout: 10000 },
  async (t) => {
    const barrier = new Int32Array(new SharedArrayBuffer(8));
    const script = `
    import { parentPort, workerData } from "node:worker_threads";
    import { run } from ${JSON.stringify(new URL("./net.mjs", import.meta.url).href)};
    parentPort.on("message", async ({ handle }) => {
      const barrier = workerData.barrier;
      Atomics.store(barrier, 0, 1);
      Atomics.notify(barrier, 0);
      Atomics.wait(barrier, 1, 0);
      parentPort.postMessage(await run({ op: "accept", handle }));
    });
  `;
    const call = netWorker(t, {
      url: new URL("data:text/javascript," + encodeURIComponent(script)),
      data: { barrier },
    });
    const { ok: listener } = call({
      op: "listen",
      host: "127.0.0.1",
      port: 0,
      address: "loopback",
    });
    call({ op: "set_timeout", handle: listener, timeout: 0 });
    const {
      ok: [, port],
    } = call({ op: "local_address", handle: listener });
    const outcome = once(call.worker, "message");
    call.worker.postMessage({ handle: listener });
    assert.notEqual(
      Atomics.wait(barrier, 0, 0, 5000),
      "timed-out",
      "worker entered the callback barrier",
    );
    const peer = net.connect({ host: "127.0.0.1", port });
    t.after(() => peer.destroy());
    await once(peer, "connect");
    Atomics.store(barrier, 1, 1);
    Atomics.notify(barrier, 1);
    const [result] = await outcome;
    assert.equal(typeof result.ok, "number", JSON.stringify(result));
    call({ op: "close", handle: result.ok });
    call({ op: "close", handle: listener });
  },
);

// #726: the HTTP worker owns a request until its final body or abort.
async function httpLoopback(t, handler) {
  const sockets = new Set();
  const server = http.createServer(handler);
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("error", () => {});
    socket.once("close", () => sockets.delete(socket));
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  t.after(() => {
    for (const socket of sockets) {
      socket.destroy();
    }
    server.close();
  });
  return `http://127.0.0.1:${server.address().port}`;
}

function httpWorker(t) {
  const call = netWorker(t, { url: new URL("./http.mjs", import.meta.url) });
  const start = (url, options = {}) => {
    const result = call({
      op: "start",
      method: "GET",
      url,
      headers: [],
      body: new Uint8Array(),
      redirects: 5,
      grants: {},
      ...options,
    });
    assert.equal(typeof result.ok, "number", "start returns an operation without waiting for HTTP");
    return result.ok;
  };
  const result = async (operation) => {
    let answer;
    await t.waitFor(
      () => {
        answer = call({ op: "poll", operation });
        assert.notEqual(answer.pending, true, "the HTTP operation is ready");
      },
      { interval: 1, timeout: 5000 },
    );
    return answer;
  };
  return { call, start, result };
}

for (const phase of ["headers", "body", "redirect"]) {
  test(`HTTP abort closes a request waiting for ${phase}`, { timeout: 10000 }, async (t) => {
    let seen;
    const received = new Promise((resolve) => {
      seen = resolve;
    });
    const paths = [];
    const url = await httpLoopback(t, (request, response) => {
      paths.push(request.url);
      if (phase === "redirect" && request.url === "/") {
        response.writeHead(302, { Location: "/slow" });
        response.end();
        return;
      }
      if (phase === "body") {
        response.writeHead(200, { "Content-Type": "text/plain" });
        response.write("unfinished body");
      }
      seen(request);
    });
    const { call, start } = httpWorker(t);
    const operation = start(url);
    const request = await received;
    assert.deepEqual(call({ op: "poll", operation }), { pending: true });
    const closed = once(request.socket, "close");
    assert.deepEqual(call({ op: "abort", operation }), { ok: null });
    await closed;
    assert.deepEqual(call({ op: "open" }), { ok: 0 });
    assert.deepEqual(call({ op: "abort", operation }), { ok: null }, "abort is idempotent");
    assert.deepEqual(paths, phase === "redirect" ? ["/", "/slow"] : ["/"]);
  });
}

test(
  "HTTP operations complete independently and abort never posts a stray reply",
  { timeout: 10000 },
  async (t) => {
    let slow;
    const received = new Promise((resolve) => {
      slow = resolve;
    });
    const url = await httpLoopback(t, (request, response) => {
      if (request.url === "/slow") {
        slow(request);
        return;
      }
      response.writeHead(201, ["X-Repeat", "one", "X-Repeat", "two"]);
      response.end(request.url);
    });
    const { call, start, result } = httpWorker(t);
    const pending = start(`${url}/slow`);
    const request = await received;
    const fast = start(`${url}/fast`);
    const closed = once(request.socket, "close");
    call({ op: "abort", operation: pending });
    await closed;
    const answer = await result(fast);
    assert.equal(answer.status, 201, JSON.stringify(answer));
    assert.equal(new TextDecoder().decode(answer.body), "/fast");
    assert.equal(answer.url, `${url}/fast`);
    assert.deepEqual(
      answer.headers.filter(([name]) => name === "x-repeat"),
      [
        ["x-repeat", "one"],
        ["x-repeat", "two"],
      ],
    );
    assert.deepEqual(call({ op: "open" }), { ok: 0 });
    const next = await result(start(`${url}/next`));
    assert.equal(
      new TextDecoder().decode(next.body),
      "/next",
      "the next reply belongs to its request",
    );
    assert.deepEqual(call({ op: "open" }), { ok: 0 });
  },
);

test("HTTP grant and URL errors finish before any network wait", async (t) => {
  const { call, start } = httpWorker(t);
  const invalid = start("not a URL");
  assert.deepEqual(call({ op: "poll", operation: invalid }), {
    error: "InvalidUrl",
    text: "not a URL",
  });
  const denied = start("http://127.0.0.1:1/", { grants: { Http: false } });
  assert.deepEqual(call({ op: "poll", operation: denied }), {
    error: "NotGranted",
    text: "127.0.0.1",
  });
  assert.deepEqual(call({ op: "open" }), { ok: 0 });
});

// #950: a derived provider's transport settings on the Node host. The TLS
// fixtures are test-only keys: a CA, a server certificate for 127.0.0.1
// that it signed, and a client identity that it signed.
const tlsFixture = (name) =>
  readFileSync(new URL(`./fixtures/tls/${name}.pem`, import.meta.url), "utf8");
const settings = (changes = {}) => ({
  dnsServers: null,
  tlsRoots: [],
  tlsIdentity: null,
  connectTimeout: null,
  tlsHandshakeTimeout: null,
  responseHeaderTimeout: null,
  bodyReadTimeout: null,
  ...changes,
});

async function httpsLoopback(t, options, handler) {
  const server = https.createServer(
    { cert: tlsFixture("server"), key: tlsFixture("server-key"), ...options },
    handler,
  );
  server.on("tlsClientError", () => {});
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => {
    server.closeAllConnections?.();
    server.close();
  });
  return `https://127.0.0.1:${server.address().port}`;
}

test("an HTTP setting the Node host does not honour is refused before any work", (t) => {
  const { call, start } = httpWorker(t);
  const refused = start("http://127.0.0.1:1/", {
    grants: { Http: false },
    settings: settings({ dnsServers: ["10.0.0.2"], connectTimeout: 1000 }),
  });
  assert.deepEqual(call({ op: "poll", operation: refused }), {
    error: "UnsupportedSetting",
    text: "resolver",
  });
});

test(
  "HTTP phase timeouts bound the header wait and the body read",
  { timeout: 10000 },
  async (t) => {
    const url = await httpLoopback(t, (request, response) => {
      if (request.url === "/body") {
        response.writeHead(200, { "Content-Length": "10" });
        response.write("part");
      }
    });
    const { start, result } = httpWorker(t);
    for (const [path, change] of [
      ["/headers", { responseHeaderTimeout: 100 }],
      ["/body", { bodyReadTimeout: 100 }],
    ]) {
      const started = performance.now();
      const answer = await result(start(`${url}${path}`, { settings: settings(change) }));
      assert.deepEqual(answer, { error: "Timeout", text: null }, path);
      assert.ok(performance.now() - started < 3000, `${path} stops long before the 30 s bound`);
    }
  },
);

test("a zero connect timeout expires before any connection", async (t) => {
  let connections = 0;
  const server = net.createServer((socket) => {
    connections += 1;
    socket.destroy();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const { start, result } = httpWorker(t);
  const url = `http://127.0.0.1:${server.address().port}/`;
  const answer = await result(start(url, { settings: settings({ connectTimeout: 0 }) }));
  assert.deepEqual(answer, { error: "Timeout", text: null });
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(connections, 0);
});

test("each redirect hop restarts the phase timers", { timeout: 10000 }, async (t) => {
  const url = await httpLoopback(t, (request, response) => {
    setTimeout(() => {
      if (request.url === "/end") {
        response.end("done");
      } else {
        response.writeHead(302, { Location: request.url === "/" ? "/next" : "/end" });
        response.end();
      }
    }, 150);
  });
  const { start, result } = httpWorker(t);
  // Three hops of 150 ms each outlast one 250 ms header timer; the connect
  // timer ends at each connection.
  const answer = await result(
    start(url, { settings: settings({ connectTimeout: 50, responseHeaderTimeout: 250 }) }),
  );
  assert.equal(answer.status, 200, JSON.stringify(answer));
  assert.equal(new TextDecoder().decode(answer.body), "done");
});

test("a TLS handshake timeout stops a peer that never answers the hello", async (t) => {
  const server = net.createServer((socket) => socket.on("error", () => {}));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const { start, result } = httpWorker(t);
  const answer = await result(
    start(`https://127.0.0.1:${server.address().port}/`, {
      settings: settings({ tlsHandshakeTimeout: 100 }),
    }),
  );
  assert.deepEqual(answer, { error: "Timeout", text: null });
});

test("extra TLS roots extend the store and an identity reaches the server", async (t) => {
  const url = await httpsLoopback(
    t,
    { ca: tlsFixture("ca"), requestCert: true, rejectUnauthorized: false },
    (request, response) => {
      const peer = request.socket.getPeerCertificate();
      response.end(request.socket.authorized ? `client ${peer.subject.CN}` : "anonymous");
    },
  );
  const { start, result } = httpWorker(t);
  const untrusted = await result(start(url));
  assert.equal(untrusted.error, "Tls", JSON.stringify(untrusted));
  const trusted = await result(
    start(url, { settings: settings({ tlsRoots: [tlsFixture("ca")] }) }),
  );
  assert.equal(new TextDecoder().decode(trusted.body), "anonymous", JSON.stringify(trusted));
  const identity = {
    certificateChain: tlsFixture("client"),
    privateKey: tlsFixture("client-key"),
  };
  const mutual = await result(
    start(url, { settings: settings({ tlsRoots: [tlsFixture("ca")], tlsIdentity: identity }) }),
  );
  assert.equal(new TextDecoder().decode(mutual.body), "client hd test client");
});

test("a TLS key Node cannot load is a TLS failure", async (t) => {
  const url = await httpsLoopback(t, {}, (request, response) => response.end());
  const { start, result } = httpWorker(t);
  const answer = await result(
    start(url, {
      settings: settings({
        tlsRoots: [tlsFixture("ca")],
        tlsIdentity: { certificateChain: tlsFixture("client"), privateKey: tlsFixture("ca") },
      }),
    }),
  );
  assert.equal(answer.error, "Tls", JSON.stringify(answer));
});

for (const mode of ["expired deadline", "host close"]) {
  test(`the real Node host aborts pending HTTP after ${mode}`, { timeout: 10000 }, async (t) => {
    let seen;
    const received = new Promise((resolve) => {
      seen = resolve;
    });
    const url = await httpLoopback(t, (request) => {
      seen(request);
    });
    const host = createHost({ out() {}, err() {} });
    t.after(() => host.close());
    const memory = new WebAssembly.Memory({ initial: 1 });
    host.prime({ exports: { "hd.x": memory } });
    const encoded = new Enc();
    METHODS["hd:Http"].send.params[0].enc(encoded, {
      method: { tag: "Get" },
      url,
      headers: [],
      body: new Uint8Array(),
      redirects: 5,
      dns_servers: null,
      tls_roots: [],
      tls_identity: null,
      connect_timeout: null,
      tls_handshake_timeout: null,
      response_header_timeout: null,
      body_read_timeout: null,
    });
    new Uint8Array(memory.buffer).set(encoded.done());
    const [status, operation] = host.imports["hd:Http"]["send.start"](encoded.len);
    assert.equal(status, 1, "send stays pending while the server has not replied");
    const request = await received;
    const closed = once(request.socket, "close");
    if (mode === "expired deadline") {
      host.core.st.deadline = performance.now() - 1;
      host.imports["hd:rt"].abort(operation);
    } else {
      host.close();
    }
    await closed;
    assert.equal(host.core.st.ops.size, 0);
  });
}

test("the shared HTTP adapter preserves synchronous browser-style answers", () => {
  const request = {
    method: { tag: "Other", name: "PROPFIND" },
    url: "https://example.test/",
    headers: [["X-Request", "one"]],
    body: Uint8Array.of(1, 2),
    redirects: 2,
  };
  const response = { status: 207, headers: [], body: Uint8Array.of(3), url: request.url };
  const provider = httpProvider((sent) => {
    assert.deepEqual(sent, { ...request, method: "PROPFIND", settings: httpSettings(request) });
    return response;
  });
  assert.deepEqual(provider.send(request), Ok(response));
  const failed = httpProvider(() => ({ error: "Timeout" })).send(request);
  assert.equal(failed.ok, false);
  assert.equal(failed.error.tag, "Timeout");
});

test("the browser host refuses every transport setting before any request", () => {
  const sent = {
    method: "GET",
    url: "https://example.test/",
    headers: [],
    body: new Uint8Array(),
    redirects: 1,
    timeout: null,
  };
  assert.deepEqual(
    browserSend({ ...sent, settings: settings({ bodyReadTimeout: 0, tlsRoots: ["root"] }) }),
    { error: "UnsupportedSetting", text: "tls.roots" },
  );
});

// #812: real children live on a worker even when the guest uses rt.block.
function processHost(t, env) {
  const api = netHost(t, "hd:Process", {
    processLauncher: process.env.HD_PROCESS_LAUNCHER,
    ...env,
  });
  const run = (script, options = {}) =>
    api.call("run", {
      program: process.execPath,
      args: ["-e", script],
      env: [],
      current_dir: null,
      stdin: "",
      ...options,
    });
  return { ...api, run };
}

test("Process pending child lets sibling clocks advance before release", async (t) => {
  const root = fixture(t, {});
  const ready = join(root, "ready"),
    release = join(root, "release");
  const { host, run, take } = processHost(t);
  const child = run(`const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(ready)},'');
    const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(release)})){
      clearInterval(timer);process.stdout.write('released');}},5);`);
  assert.equal(typeof child.operation, "number");
  await t.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
  for (let tick = 0; tick < 3; tick++) {
    const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
    assert.ok(host.imports["hd:rt"].block() > 0);
    host.imports["hd:Clock"]["sleep.finish"](clock);
    assert.equal(host.core.st.ops.get(child.operation).completed, false);
  }
  writeFileSync(release, "");
  assert.deepEqual(
    take("run", child),
    Ok({ stdout: "released", stderr: "", status: 0, program: process.execPath }),
  );
});

test("Process drains both pipes while feeding input and keeps nonzero status", (t) => {
  const { run, take } = processHost(t);
  // The exchange buffer belongs to the guest; this larger command needs more.
  const input = "A".repeat(50_000);
  const result = take(
    "run",
    run(
      `const fs=require('node:fs');
    fs.writeSync(1,'x'.repeat(70000));fs.writeSync(2,'y'.repeat(70000));
    const input=fs.readFileSync(0);fs.writeSync(1,String(input.length));process.exitCode=23;`,
      { stdin: input },
    ),
  );
  assert.equal(result.ok, true);
  assert.equal(result.value.status, 23);
  assert.equal(result.value.program, process.execPath);
  assert.equal(result.value.stdout.length, 70_005);
  assert.equal(result.value.stderr.length, 70_000);
  assert.ok(result.value.stdout === "x".repeat(70_000) + "50000");
  assert.ok(result.value.stderr === "y".repeat(70_000));
});

test("Process child closing stdin preserves output and status", (t) => {
  const { run, take } = processHost(t);
  // Input exceeds a pipe buffer; the child closes without reading it.
  assert.deepEqual(
    take(
      "run",
      run(
        `require('node:fs').closeSync(0);process.stdout.write('ok');process.stderr.write('err');process.exitCode=7;`,
        {
          stdin: "line\n".repeat(200_000),
        },
      ),
    ),
    Ok({ stdout: "ok", stderr: "err", status: 7, program: process.execPath }),
  );
});

test("Process missing program, grant and malformed commands keep typed errors", (t) => {
  const api = processHost(t);
  assert.deepEqual(
    api.take("run", api.run("", { program: "hd-missing-muse-812" })),
    Err({ tag: "NotFound", program: "hd-missing-muse-812" }),
  );
  const denied = processHost(t, { grants: { Process: false } });
  const refused = denied.run("", { program: "hd-missing-muse-812" });
  assert.equal(refused.operation, undefined);
  assert.deepEqual(refused.result, Err({ tag: "NotGranted" }));
  const bad = api.run("", { args: ["bad\0argument"] });
  assert.equal(bad.operation, undefined);
  assert.equal(bad.result.ok, false);
  assert.equal(bad.result.error.tag, "Other");
});

function processWorker(t) {
  const call = netWorker(t, { url: new URL("./process.mjs", import.meta.url), close: "close" });
  const start = (script, more = {}) =>
    call({
      op: "wait_start",
      program: process.execPath,
      args: ["-e", script],
      cwd: process.cwd(),
      // The provider supplies the full environment; Rust does not apply
      // libuv's implicit Windows essential-variable injection.
      env: { ...process.env },
      input: "",
      launcher: process.env.HD_PROCESS_LAUNCHER,
      timeout: Infinity,
      ...more,
    }).ok;
  return { call, start };
}

for (const phase of ["running", "leader-exited"]) {
  test(
    `Process abort kills its process group (${phase})`,
    { skip: process.platform === "win32" },
    async (t) => {
      const root = fixture(t, {}),
        ready = join(root, "ready"),
        parent = join(root, "parent");
      const { call, start } = processWorker(t);
      const descendant = `require('node:fs').writeFileSync(${JSON.stringify(ready)},String(process.pid));setInterval(()=>{},1000)`;
      const script = `require('node:fs').writeFileSync(${JSON.stringify(parent)},String(process.pid));const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e',${JSON.stringify(descendant)}],{stdio:['ignore','inherit','inherit']});
      ${phase === "leader-exited" ? "child.unref();" : "setInterval(()=>{},1000);"}`;
      const operation = start(script);
      await t.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
      const pid = Number(readFileSync(ready, "utf8"));
      const parentPid = Number(readFileSync(parent, "utf8"));
      if (phase === "leader-exited") {
        await t.waitFor(() => assert.throws(() => process.kill(parentPid, 0)), { timeout: 5000 });
      } else {
        process.kill(parentPid, 0);
      }
      t.after(() => {
        try {
          process.kill(pid, "SIGKILL");
        } catch {}
      });
      assert.deepEqual(call({ op: "wait_poll", operation }), { pending: true });
      assert.deepEqual(call({ op: "wait_abort", operation }), { ok: null });
      assert.deepEqual(call({ op: "wait_abort", operation }), { ok: null });
      await t.waitFor(() => assert.throws(() => process.kill(pid, 0)), { timeout: 5000 });
    },
  );
}

test("Process ready result stays owned until finish or abort", async (t) => {
  const { call, start } = processWorker(t);
  const operation = start("process.stdout.write('ready')");
  let result;
  await t.waitFor(
    () => {
      result = call({ op: "wait_poll", operation });
      assert.ok(result.ok);
    },
    { timeout: 5000 },
  );
  assert.deepEqual(call({ op: "wait_poll", operation }), result);
  assert.deepEqual(call({ op: "wait_abort", operation }), { ok: null });
  const second = start("process.stdout.write('next')");
  await t.waitFor(() => assert.ok(call({ op: "wait_poll", operation: second }).ok), {
    timeout: 5000,
  });
  assert.deepEqual(call({ op: "wait_finish", operation: second }), { ok: null });
});

test("Process prepared child uses its configured launcher, directory and environment", (t) => {
  const root = fixture(t, {
    "child.mjs": `process.stdout.write(JSON.stringify([process.argv.slice(2),process.cwd(),process.env.MUSE_CHILD]));`,
  });
  const api = processHost(t, {
    programs: {
      prepared: {
        host: join(root, "child.mjs"),
        wasm: "child.wasm",
        config: "child.json",
        cwd: root,
      },
    },
  });
  const output = api.take(
    "run",
    api.run("", {
      program: "prepared",
      args: ["one", "two"],
      env: [
        ["MUSE_CHILD", "first"],
        ["MUSE_CHILD", "last"],
      ],
    }),
  );
  assert.equal(output.ok, true);
  assert.equal(output.value.program, "prepared");
  assert.deepEqual(JSON.parse(output.value.stdout), [
    ["child.wasm", "child.json", "one", "two"],
    realpathSync(root),
    "last",
  ]);
});

test("Process worker deadline kills the child and reports timeout", async (t) => {
  const { call, start } = processWorker(t);
  const operation = start("setInterval(()=>{},1000)", { timeout: 50 });
  await t.waitFor(() => assert.deepEqual(call({ op: "wait_poll", operation }), { timeout: true }), {
    timeout: 5000,
  });
  assert.deepEqual(call({ op: "wait_finish", operation }), { ok: null });
});

test("Process host close cancels a child with unread output", async (t) => {
  const root = fixture(t, {}),
    ready = join(root, "ready");
  const { host, run } = processHost(t);
  run(
    `require('node:fs').writeFileSync(${JSON.stringify(ready)},String(process.pid));setInterval(()=>{},1000)`,
  );
  await t.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
  const pid = Number(readFileSync(ready, "utf8"));
  t.after(() => {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  });
  host.close();
  await t.waitFor(() => assert.throws(() => process.kill(pid, 0)), { timeout: 5000 });
});

// A host timeout longer than Node's timer range must not turn into 1 ms.
test("Process long deadline does not overflow the worker timer", async (t) => {
  const { call, start } = processWorker(t);
  const operation = start("setTimeout(()=>process.stdout.write('ok'),20)", {
    timeout: 2_147_483_648,
  });
  await t.waitFor(
    () => {
      const result = call({ op: "wait_poll", operation });
      assert.equal(result.ok?.stdout, "ok");
    },
    { timeout: 5000 },
  );
  call({ op: "wait_finish", operation });
});

