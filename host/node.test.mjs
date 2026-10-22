import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import net from "node:net";
import http from "node:http";
import { once } from "node:events";
import { MessageChannel, receiveMessageOnPort, Worker } from "node:worker_threads";
import { createCoreHost, httpProvider, pendingOperation } from "./core.mjs";
import { Enc, Ok } from "./wire.mjs";
import { METHODS } from "./gen/abi.mjs";

import { cgroupCpuCount, createHost } from "./node.mjs";

function fixture(t, files) {
  const root = mkdtempSync(join(tmpdir(), "hd-cgroup-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
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
function operationHost(ready, onDone, onAbort) {
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
    port1.close();
    await worker.terminate();
  });
  const call = (request) => {
    Atomics.store(flag, 0, 0);
    port1.postMessage(request);
    assert.notEqual(Atomics.wait(flag, 0, 0, 5000), "timed-out", "worker control reply");
    return receiveMessageOnPort(port1).message;
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
  await t.waitFor(() => call({ op: "open" }).ok.stream === 1, { interval: 1, timeout: 5000 });
  assert.equal(call({ op: "open" }).ok.stream, 1);
  call({ op: "connect_abort", operation });
  assert.equal(call({ op: "open" }).ok.stream, 0);
});

test("worker result transfers one ready stream and close releases it", async (t) => {
  const address = await loopback(t),
    call = netWorker(t);
  const { ok: operation } = call({ op: "connect_start", ...address });
  await t.waitFor(() => call({ op: "open" }).ok.stream === 1, { interval: 1, timeout: 5000 });
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
