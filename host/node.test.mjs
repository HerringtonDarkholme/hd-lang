import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import net from "node:net";
import { MessageChannel, receiveMessageOnPort, Worker } from "node:worker_threads";
import { createCoreHost, pendingOperation } from "./core.mjs";
import { Ok } from "./wire.mjs";

import { cgroupCpuCount } from "./node.mjs";

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

function netWorker(t) {
  const { port1, port2 } = new MessageChannel();
  const flag = new Int32Array(new SharedArrayBuffer(4));
  const worker = new Worker(new URL("./net.mjs", import.meta.url), {
    workerData: { port: port2, flag },
    transferList: [port2],
  });
  t.after(async () => {
    port1.close();
    await worker.terminate();
  });
  return (request) => {
    Atomics.store(flag, 0, 0);
    port1.postMessage(request);
    assert.notEqual(Atomics.wait(flag, 0, 0, 5000), "timed-out", "worker control reply");
    return receiveMessageOnPort(port1).message;
  };
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
