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
import { once } from "node:events";
import { MessageChannel, Worker } from "node:worker_threads";
import { createCoreHost, httpProvider, pendingOperation, TimeLimit } from "./core.mjs";
import { Dec, Enc, Err, Ok, str } from "./wire.mjs";
import { METHODS } from "./gen/abi.mjs";

import { cgroupCpuCount, createHost, syncClient } from "./node.mjs";
import { createOutput } from "./output.mjs";
import { postReply, receiveReply } from "./worker-control.mjs";

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
          "hd:Console": {
            write: () =>
              pendingOperation(
                ready,
                () => {
                  onDone();
                  return Ok(undefined);
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
  const start = () => host.imports["hd:Console"]["write.start"](0);
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
  host.imports["hd:Console"]["write.finish"](pending);
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
  host.imports["hd:Console"]["write.finish"](pending);
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
  assert.throws(() => host.imports["hd:Console"]["write.finish"](operation));
  assert.equal(delivered, 0, "an unfinished result cannot transfer ownership");
  host.imports["hd:rt"].block();
  assert.equal(delivered, 0, "completion is not guest delivery");
  host.imports["hd:Console"]["write.finish"](operation);
  assert.equal(delivered, 1);
  assert.throws(() => host.imports["hd:Console"]["write.finish"](operation));
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
        host.imports["hd:Console"]["write.finish"](pending);
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
    if (options.close) call({ op: options.close });
    port1.close();
    await worker.terminate();
  });
  let nextCall = 1;
  const call = (request) => {
    const id = nextCall++;
    const deadline = performance.now() + 5000;
    Atomics.store(flag, 0, 0);
    port1.postMessage({ id, request });
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
    for (const socket of sockets) socket.destroy();
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
        if (typeof param === "string") raw.push(args[i]);
        else param.enc(encoded, args[i]);
      });
      if (raw.length !== method.params.length) {
        new Uint8Array(memory.buffer).set(encoded.done());
        raw.push(encoded.len);
      }
    }
    if (method.wait === "never") return { result: decode(name, imports[name](...raw)) };
    const [status, value] = imports[`${name}.start`](...raw);
    return status === 0 ? { result: decode(name, value) } : { operation: value };
  };
  const take = (name, started) => {
    if (started.result) return started.result;
    if (!host.core.st.ops.get(started.operation).completed) host.imports["hd:rt"].block();
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
    if (phase === "completed past deadline") host.core.st.deadline = performance.now() - 1;
    host.imports["hd:rt"].abort(first.operation);
    host.core.st.deadline = deadline;
    if (phase === "pending") peer.write("abc");
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
      if (reversed) operations.reverse();
      for (const operation of operations) host.imports["hd:rt"].abort(operation);
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
    for (const socket of sockets) socket.destroy();
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
      query: [],
      headers: [],
      body: new Uint8Array(),
      redirects: 5,
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
    assert.deepEqual(sent, { ...request, method: "PROPFIND" });
    return response;
  });
  assert.deepEqual(provider.send(request), Ok(response));
  const failed = httpProvider(() => ({ error: "Timeout" })).send(request);
  assert.equal(failed.ok, false);
  assert.equal(failed.error.tag, "Timeout");
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
      } else process.kill(parentPid, 0);
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

// #814: a held resolver lives only in the test worker; the provider and
// transport are unmodified production sources copied beside that wrapper.
async function heldLookupHost(t) {
  const root = fixture(t, {});
  for (const name of [
    "node.mjs",
    "core.mjs",
    "output.mjs",
    "worker-control.mjs",
    "checked-report.mjs",
    "float.mjs",
    "journal.mjs",
    "wire.mjs",
    "package-providers.mjs",
    "gen/abi.mjs",
  ]) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    copyFileSync(new URL(`./${name}`, import.meta.url), join(root, name));
  }
  const entered = join(root, "entered"),
    release = join(root, "release"),
    finished = join(root, "finished");
  writeFileSync(
    join(root, "net.mjs"),
    `
    import {promises as dns} from 'node:dns';
    import {existsSync,writeFileSync} from 'node:fs';
    const lookup=dns.lookup;
    dns.lookup=async(host,options)=>{
      if(host!=='slow.example')return lookup(host,options);
      writeFileSync(${JSON.stringify(entered)},'');
      await new Promise(resolve=>{const timer=setInterval(()=>{if(existsSync(${JSON.stringify(release)})){clearInterval(timer);resolve();}},1);});
      writeFileSync(${JSON.stringify(finished)},'');
      const address={address:'127.0.0.1',family:4};
      return options.all?[address]:address;
    };
    await import(${JSON.stringify(new URL("./net.mjs", import.meta.url).href)});
  `,
  );
  const module = await import(pathToFileURL(join(root, "node.mjs")).href);
  return { ...netHost(t, "hd:Net", {}, module.createHost), entered, release, finished };
}

for (const abort of [false, true]) {
  test(`Net slow lookup lets a sibling clock run (${abort ? "abort" : "finish"})`, async (t) => {
    const { host, call, take, entered, release, finished } = await heldLookupHost(t);
    const lookup = call("lookup", "slow.example");
    assert.equal(typeof lookup.operation, "number");
    assert.ok(existsSync(entered));
    for (let tick = 0; tick < 3; tick++) {
      const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
      assert.ok(host.imports["hd:rt"].block() > 0);
      host.imports["hd:Clock"]["sleep.finish"](clock);
      assert.equal(host.core.st.ops.get(lookup.operation).completed, false);
    }
    if (abort) host.imports["hd:rt"].abort(lookup.operation);
    writeFileSync(release, "");
    if (abort) {
      await t.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
      assert.deepEqual(take("lookup", call("lookup", "127.0.0.1")), Ok(["127.0.0.1"]));
    } else assert.deepEqual(take("lookup", lookup), Ok(["127.0.0.1"]));
  });
}

async function finishWrite(t, api, started) {
  if (started.result) return started.result;
  await t.waitFor(
    () => {
      const [, clock] = api.host.imports["hd:Clock"]["sleep.start"](0n);
      api.host.imports["hd:rt"].block();
      api.host.imports["hd:Clock"]["sleep.finish"](clock);
      assert.equal(api.host.core.st.ops.get(started.operation).completed, true);
    },
    { timeout: 5000, interval: 1 },
  );
  return api.take("TcpStream.write", started);
}

for (const abort of [false, true]) {
  test(
    `Net backpressured write keeps siblings running (${abort ? "abort" : "finish"})`,
    { timeout: 10000 },
    async (t) => {
      const api = await tcpHost(t);
      const { host, call, handle, peer } = api;
      peer.pause();
      host.core.st.memory.grow(256);
      const bytes = Buffer.alloc(16 * 1024 * 1024);
      for (let i = 0; i < bytes.length; i++) bytes[i] = i % 251;
      const started = call("TcpStream.write", handle, bytes);
      assert.equal(typeof started.operation, "number");
      for (let tick = 0; tick < 3; tick++) {
        const [, clock] = host.imports["hd:Clock"]["sleep.start"](0n);
        host.imports["hd:rt"].block();
        host.imports["hd:Clock"]["sleep.finish"](clock);
        assert.equal(host.core.st.ops.get(started.operation).completed, false);
      }
      const chunks = [];
      let size = 0;
      peer.on("data", (chunk) => {
        chunks.push(chunk);
        size += chunk.length;
      });
      if (abort) host.imports["hd:rt"].abort(started.operation);
      peer.resume();
      if (abort) {
        const tail = Buffer.from("after-abort");
        assert.deepEqual(
          await finishWrite(t, api, call("TcpStream.write", handle, tail)),
          Ok(undefined),
        );
        await t.waitFor(
          () => assert.ok(Buffer.concat(chunks).subarray(-tail.length).equals(tail)),
          { timeout: 5000 },
        );
        assert.ok(
          size < bytes.length + tail.length,
          "abort stops submitting the rest and keeps the stream usable",
        );
      } else {
        assert.deepEqual(await finishWrite(t, api, started), Ok(undefined));
        await t.waitFor(() => assert.equal(size, bytes.length), { timeout: 5000 });
        assert.ok(Buffer.concat(chunks).equals(bytes), "all bytes arrive in order");
      }
    },
  );
}

for (const kind of ["listen", "bind_udp"]) {
  test(`Net ${kind} abort releases a ready undelivered handle`, async (t) => {
    const call = netWorker(t);
    const operation = call({
      op: "wait_start",
      kind,
      host: "127.0.0.1",
      port: 0,
      address: "127.0.0.1:0",
    }).ok;
    let result;
    await t.waitFor(
      () => {
        result = call({ op: "wait_poll", operation });
        assert.equal(typeof result.ok, "number");
      },
      { timeout: 5000 },
    );
    assert.deepEqual(
      call({ op: "wait_poll", operation }),
      result,
      "poll does not consume the handle",
    );
    call({ op: "wait_abort", operation });
    assert.deepEqual(call({ op: "check", handle: result.ok }), { unknown: true });
    assert.deepEqual(call({ op: "open" }), { ok: { stream: 0, listener: 0, udp: 0 } });
  });
}

for (const kind of ["listen", "bind_udp"]) {
  test(`Net ${kind} abort before its callback leaves no late handle`, async (t) => {
    const root = fixture(t, {}),
      ready = join(root, "ready"),
      release = join(root, "release"),
      finished = join(root, "finished");
    const url = join(root, "held.mjs");
    writeFileSync(
      url,
      `
      import net from 'node:net';import dgram from 'node:dgram';
      import {existsSync,writeFileSync} from 'node:fs';
      const proto=${kind === "listen" ? "net.Server.prototype" : "dgram.Socket.prototype"};
      const method=${JSON.stringify(kind === "listen" ? "listen" : "bind")};
      const original=proto[method];
      proto[method]=function(...args){
        const callback=args.pop();
        return original.call(this,...args,()=>{
          writeFileSync(${JSON.stringify(ready)},String(this.address().port));
          const timer=setInterval(()=>{if(existsSync(${JSON.stringify(release)})){
            clearInterval(timer);callback();writeFileSync(${JSON.stringify(finished)},'');}},1);
        });
      };
      await import(${JSON.stringify(new URL("./net.mjs", import.meta.url).href)});
    `,
    );
    const call = netWorker(t, { url: pathToFileURL(url) });
    const operation = call({
      op: "wait_start",
      kind,
      host: "127.0.0.1",
      port: 0,
      address: "127.0.0.1:0",
    }).ok;
    await t.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
    assert.deepEqual(call({ op: "wait_poll", operation }), { pending: true });
    call({ op: "wait_abort", operation });
    const port = Number(readFileSync(ready, "utf8"));
    if (kind === "listen") {
      const replacement = net.createServer();
      await new Promise((resolve, reject) => {
        replacement.once("error", reject);
        replacement.listen(port, "127.0.0.1", resolve);
      });
      await new Promise((resolve) => replacement.close(resolve));
    } else {
      const replacement = dgram.createSocket("udp4");
      await new Promise((resolve, reject) => {
        replacement.once("error", reject);
        replacement.bind(port, "127.0.0.1", resolve);
      });
      await new Promise((resolve) => replacement.close(resolve));
    }
    writeFileSync(release, "");
    await t.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
    assert.deepEqual(call({ op: "open" }), { ok: { stream: 0, listener: 0, udp: 0 } });
  });
}

test("Net UDP abort while DNS is held sends no late datagram", async (t) => {
  const api = await heldLookupHost(t);
  const { host, call, take, entered, release, finished } = api;
  const bound = take("bind_udp", call("bind_udp", "127.0.0.1", 0));
  assert.ok(bound.ok);
  const target = dgram.createSocket("udp4");
  await new Promise((resolve) => target.bind(0, "127.0.0.1", resolve));
  t.after(() => target.close());
  let arrivals = 0;
  target.on("message", () => {
    arrivals++;
  });
  const send = call(
    "UdpSocket.send_to",
    bound.value,
    "slow.example",
    target.address().port,
    Buffer.from("cancelled"),
  );
  assert.equal(typeof send.operation, "number");
  assert.ok(existsSync(entered));
  host.imports["hd:rt"].abort(send.operation);
  writeFileSync(release, "");
  await t.waitFor(() => assert.ok(existsSync(finished)), { timeout: 5000 });
  // A successor datagram proves the original socket remains usable and
  // all already queued receive events have an observation barrier.
  const received = once(target, "message");
  const next = call(
    "UdpSocket.send_to",
    bound.value,
    "127.0.0.1",
    target.address().port,
    Buffer.from("next"),
  );
  assert.deepEqual(take("UdpSocket.send_to", next), Ok(undefined));
  const [bytes] = await received;
  assert.equal(bytes.toString(), "next");
  assert.equal(arrivals, 1);
});

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
    for (const operation of operations)
      assert.equal(api.host.core.st.ops.get(operation).completed, false);
  }
}
function finishFile(api, name, pending) {
  while (!api.host.core.st.ops.get(pending.operation).completed) api.host.imports["hd:rt"].block();
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
  ])
    assert.deepEqual(api.call("read_text", path), { result: Err({ tag, path }) });
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
  while (!api.host.core.st.ops.get(pending.operation).completed) api.host.imports["hd:rt"].block();
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
    ["-e", source, fileURLToPath(new URL("./fs.mjs", import.meta.url)), path],
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
    for (const pid of alive()) process.kill(pid, "SIGKILL");
  });
  const until = performance.now() + 3000;
  while (alive().length && performance.now() < until)
    await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(alive(), [], "abrupt worker termination leaves no file helper process");
});

// #813: the worker reserves original bytes until guest finish.
function inputWorker(t, content) {
  const root = mkdtempSync(join(tmpdir(), "hd-input-"));
  writeFileSync(join(root, "input"), content);
  const fd = openSync(join(root, "input"), "r");
  const call = netWorker(t, { url: new URL("./input.mjs", import.meta.url) });
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
    for (const operation of reverse ? [second, first] : [first, second])
      input.call({ op: "wait_abort", operation });
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

test("Console unreadable input reports Closed", async (t) => {
  const call = netWorker(t, { url: new URL("./input.mjs", import.meta.url) });
  const operation = call({ op: "wait_start", kind: "line", fd: -1 }).ok;
  assert.deepEqual(call({ op: "wait_poll", operation }), { closed: true });
  call({ op: "wait_finish", operation });
});

// #816: the real Console ABI must preserve start-then-abort semantics.
for (const method of ["write", "write_error"]) {
  test(`Console ${method} aborted before polling writes nothing`, async (t) => {
    const root = fixture(t, { output: "" });
    const fd = openSync(join(root, "output"), "w");
    t.after(() => closeSync(fd));
    const output = createOutput();
    t.after(() => output.close());
    const host = createHost({ out() {}, err() {}, write: (_, text) => output.write(fd, text) });
    t.after(() => host.close());
    const memory = new WebAssembly.Memory({ initial: 1 });
    host.prime({ exports: { "hd.x": memory } });
    new Uint8Array(memory.buffer).set(new TextEncoder().encode("must-not-be-written"));
    const [status, operation] = host.imports["hd:Console"][`${method}.start`](19);
    assert.equal(status, 1);
    host.imports["hd:rt"].abort(operation);
    await output.close();
    assert.equal(readFileSync(join(root, "output"), "utf8"), "");
  });
}

test("Output connection close cancels its guest's unpolled write", async (t) => {
  const root = fixture(t, { output: "" });
  const fd = openSync(join(root, "output"), "w");
  t.after(() => closeSync(fd));
  const output = createOutput();
  t.after(() => output.close());
  const connection = output.connection();
  const guest = createOutput(connection);
  guest.write(fd, "must-not-be-written");
  connection.port.close();
  await output.drain();
  assert.equal(readFileSync(join(root, "output"), "utf8"), "");
});

// The queue keeps preceding writes in order. Cancellation drops an active
// chunk's unwritten suffix as well as a later chunk still waiting in the queue.
for (const abort of [false, true]) {
  test(`Output full socket keeps control live (${abort ? "abort" : "finish"})`, async (t) => {
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
      url: new URL("./output.mjs", import.meta.url),
      data: { output: true },
    });
    const fd = writer._handle.fd;
    const before = "earlier-output\n";
    call({ op: "enqueue", fd, text: before });
    const text = "abcdefg".repeat(2400000);
    const { ok: operation } = call({ op: "wait_start", fd, text });
    for (let i = 0; i < 5; i++) {
      assert.deepEqual(call({ op: "wait_poll", operation }), { pending: true });
      assert.deepEqual(call({ op: "idle" }), { ok: false });
    }
    if (abort) {
      const queued = call({ op: "wait_start", fd, text: "must-not-be-written" }).ok;
      assert.deepEqual(call({ op: "wait_poll", operation: queued }), { pending: true });
      call({ op: "wait_abort", operation: queued });
      call({ op: "wait_abort", operation });
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
    assert.equal(body, text.slice(0, body.length));
    if (abort) assert(body.length < text.length, "the unwritten suffix is discarded");
    else {
      assert.equal(body, text);
      assert.deepEqual(call({ op: "wait_poll", operation }), { result: Ok(undefined) });
      call({ op: "wait_finish", operation });
    }
  });
}

// A write larger than the pipe buffer while the reader is paused: direct
// writes come first, the full pipe starts the writer thread with the rest,
// the program keeps polling, later output stays behind it, and direct
// writes resume once the thread's queue is empty.
test(
  "Output to a held stdout pipe hands off to the thread in order",
  { skip: process.platform === "win32", timeout: 15000 },
  async (t) => {
    const url = new URL("./output.mjs", import.meta.url).href;
    const script = `
      import { writeSync } from "node:fs";
      import { createOutput } from ${JSON.stringify(url)};
      const log = (line) => writeSync(2, line + "\\n");
      const check = (ok, message) => { if (!ok) { log("FAIL " + message); process.exit(1); } };
      const output = createOutput();
      output.out("before\\n");
      check(output.write(1, "small\\n").poll(), "a direct write is ready at its first poll");
      check(!output.threaded(), "no thread before the pipe is full");
      const big = output.write(1, "x".repeat(4 << 20));
      let ticks = 0;
      await new Promise((resolve) => {
        const step = () => {
          if (big.poll()) return resolve();
          check(output.threaded(), "a full pipe hands the rest to the thread");
          if (++ticks === 1) {
            output.out("after-full\\n");
            output.err("err-after-full\\n");
          }
          log("tick " + ticks);
          setTimeout(step, 2);
        };
        step();
      });
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
        if (errors.includes("tick 20\n")) resolve();
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
