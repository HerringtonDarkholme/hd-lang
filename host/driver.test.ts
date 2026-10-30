import assert from "node:assert/strict";
import { test } from "node:test";
import { createCoreHost, pendingOperation, HostContract } from "./core.ts";
import { Enc, Ok } from "./wire.ts";
import { readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";

// A real module forwards its root ABI exports to observable callbacks.
// Completion and exchange-memory handling remain the production core's.
function instance(callbacks, resultType = 0x7f) {
  const types = new Enc().leb(3);
  types.leb(0x60).leb(0).leb(0);
  types.leb(0x60).leb(1).leb(0x7f).leb(1).leb(resultType);
  types.leb(0x60).leb(1).leb(0x7f).leb(0);
  const imports = new Enc().leb(3);
  for (const [index, name] of ["init", "poll", "wake"].entries()) {
    imports.str("test").str(name).leb(0).leb(index);
  }
  const memory = new Enc().leb(1).leb(0).leb(1);
  const exports = new Enc().leb(4);
  for (const [index, name] of ["hd.init", "hd.poll", "hd.wake"].entries()) {
    exports.str(name).leb(0).leb(index);
  }
  exports.str("hd.x").leb(2).leb(0);
  const body = new Enc()
    .leb(1)
    .bytes(types.done())
    .leb(2)
    .bytes(imports.done())
    .leb(5)
    .bytes(memory.done())
    .leb(7)
    .bytes(exports.done());
  const module = new WebAssembly.Module(
    new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, ...body.done()]),
  );
  return new WebAssembly.Instance(module, { test: { init() {}, wake() {}, ...callbacks } });
}

test("a step initializes once, publishes wakes, and retains its terminal status", () => {
  const host = createCoreHost({ out() {}, err() {} });
  const events = [];
  let handle,
    ready = false,
    first = true;
  const guest = instance({
    init() {
      events.push("init");
    },
    poll(context) {
      assert.equal(context, 0);
      events.push("poll");
      if (first) {
        first = false;
        const [pending, id] = host.imports["hd:Clock"]["sleep.start"](0n);
        assert.equal(pending, 1);
        handle = id;
        return -1;
      }
      assert.equal(ready, true);
      host.imports["hd:Clock"]["sleep.finish"](handle);
      return 7;
    },
    wake(count) {
      events.push("wake");
      assert.equal(count, 1);
      assert.equal(new DataView(guest.exports["hd.x"].buffer).getUint32(0, true), handle);
      ready = true;
    },
  });
  try {
    const driver = host.createDriver(guest);
    assert.deepEqual(events, [], "constructing the driver does not run guest code");
    assert.deepEqual(driver.step(), { status: -1, wakes: 1 });
    assert.deepEqual(driver.step(), { status: 7, wakes: 0 });
    assert.deepEqual(driver.step(), { status: 7, wakes: 0 });
    assert.deepEqual(events, ["init", "poll", "wake", "poll"]);
  } finally {
    host.close();
  }
});

test("idle turns retain pollable I/O without creating a timer or background driver", async () => {
  let ready = false,
    woken = false,
    handle,
    polls = 0,
    cancelled = 0;
  const host = createCoreHost(
    { out() {}, err() {} },
    {},
    {
      providers: () => ({
        traits: {
          "hd:ConsoleInput": {
            read_line: () =>
              pendingOperation(
                () => ready,
                () => Ok(null),
                () => {
                  cancelled++;
                },
              ),
          },
        },
      }),
    },
  );
  const guest = instance({
    poll() {
      polls++;
      if (handle === undefined) {
        new Uint8Array(guest.exports["hd.x"].buffer)[0] = 65;
        [, handle] = host.imports["hd:ConsoleInput"]["read_line.start"]();
      }
      if (!woken) {
        return -1;
      }
      host.imports["hd:ConsoleInput"]["read_line.finish"](handle);
      return 0;
    },
    wake() {
      woken = true;
    },
  });
  const timeout = globalThis.setTimeout;
  try {
    globalThis.setTimeout = () => {
      throw new Error("a step must not create a timer");
    };
    const driver = host.createDriver(guest);
    assert.deepEqual(driver.step(), { status: -1, wakes: 0 });
    assert.deepEqual(driver.step(), { status: -1, wakes: 0 });
    await Promise.resolve();
    assert.equal(polls, 2, "only explicit calls poll the instance");
    ready = true;
    assert.deepEqual(driver.step(), { status: -1, wakes: 1 });
    assert.deepEqual(driver.step(), { status: 0, wakes: 0 });
  } finally {
    globalThis.setTimeout = timeout;
    host.close();
  }
  assert.equal(cancelled, 0, "delivery transferred the completed operation");
});

test("synchronous waits accept ready work and reject a pending wait", () => {
  for (const duration of [0n, 1000000n]) {
    const host = createCoreHost({ out() {}, err() {} });
    const guest = instance({
      poll() {
        const [, id] = host.imports["hd:Clock"]["sleep.start"](duration);
        assert.equal(host.imports["hd:rt"].block(), 1);
        host.imports["hd:Clock"]["sleep.finish"](id);
        return 0;
      },
    });
    try {
      const driver = host.createDriver(guest);
      if (duration === 0n) {
        assert.deepEqual(driver.step(), { status: 0, wakes: 0 });
      } else {
        assert.throws(() => driver.step(), /synchronous host wait is pending/);
        assert.throws(() => driver.step(), /driver has failed/);
      }
    } finally {
      host.close();
    }
  }
});

test("close cancels pending work once and forbids subsequent turns", () => {
  let cancelled = 0;
  const host = createCoreHost(
    { out() {}, err() {} },
    {},
    {
      providers: () => ({
        traits: {
          "hd:ConsoleInput": {
            read_line: () =>
              pendingOperation(
                () => false,
                () => Ok(null),
                () => {
                  cancelled++;
                },
              ),
          },
        },
      }),
    },
  );
  const guest = instance({
    poll() {
      new Uint8Array(guest.exports["hd.x"].buffer)[0] = 65;
      host.imports["hd:ConsoleInput"]["read_line.start"]();
      return -1;
    },
  });
  const driver = host.createDriver(guest);
  driver.step();
  host.close();
  host.close();
  assert.equal(cancelled, 1);
  assert.throws(() => driver.step(), /instance is closed/);
  assert.throws(() => host.createDriver(guest), /instance is closed/);
});

test("a driver rejects reentrancy, including output callbacks", () => {
  let driver;
  const host = createCoreHost({
    out() {
      driver.step();
    },
    err() {},
  });
  const guest = instance({
    poll() {
      new Uint8Array(guest.exports["hd.x"].buffer)[0] = 65;
      host.imports["hd:rt"].stdout(1);
      return 0;
    },
  });
  try {
    driver = host.createDriver(guest);
    assert.throws(() => driver.step(), /reentrant instance poll/);
    assert.throws(() => driver.step(), /driver has failed/);
  } finally {
    host.close();
  }
});

test("a trap flushes earlier output and permanently fails that driver", () => {
  const output = [];
  const trap = new WebAssembly.RuntimeError("unreachable");
  const host = createCoreHost({
    out(text) {
      output.push(text);
    },
    err() {},
  });
  const guest = instance({
    poll() {
      new Uint8Array(guest.exports["hd.x"].buffer)[0] = 65;
      host.imports["hd:rt"].stdout(1);
      throw trap;
    },
  });
  try {
    const driver = host.createDriver(guest);
    assert.throws(
      () => driver.step(),
      (error) => error === trap,
    );
    assert.deepEqual(output, ["A"]);
    assert.throws(() => driver.step(), /driver has failed/);
  } finally {
    host.close();
  }
});

test("a host selects one driver mode and validates root exports before selection", async () => {
  const host = createCoreHost({ out() {}, err() {} });
  const guest = instance({ poll: () => 0 });
  try {
    assert.throws(() => host.createDriver({ exports: {} }), /missing root driver exports/);
    host.createDriver(guest);
    assert.throws(() => host.createDriver(guest), /host already has a driver/);
    await assert.rejects(host.run(guest, "hd.init", "hd.poll"), HostContract);
  } finally {
    host.close();
  }
  const asyncHost = createCoreHost({ out() {}, err() {} });
  try {
    assert.equal((await asyncHost.run(guest, "hd.init", "hd.poll")).status, 0);
    assert.throws(() => asyncHost.createDriver(guest), /host already has a driver/);
  } finally {
    asyncHost.close();
  }
});

test("a malformed root status fails the driver", () => {
  const host = createCoreHost({ out() {}, err() {} });
  const guest = instance({ poll: () => 0n }, 0x7e);
  try {
    const driver = host.createDriver(guest);
    assert.throws(() => driver.step(), /root poll returned an invalid status/);
    assert.throws(() => driver.step(), /driver has failed/);
  } finally {
    host.close();
  }
});

test("a failed completion remains owned until close and cannot be retried", () => {
  const error = new Error("provider completion failed");
  let aborted = 0;
  const host = createCoreHost(
    { out() {}, err() {} },
    {},
    {
      providers: () => ({
        traits: {
          "hd:ConsoleInput": {
            read_line: () =>
              pendingOperation(
                () => true,
                () => {
                  throw error;
                },
                () => {
                  aborted++;
                },
              ),
          },
        },
      }),
    },
  );
  const guest = instance({
    poll() {
      new Uint8Array(guest.exports["hd.x"].buffer)[0] = 65;
      host.imports["hd:ConsoleInput"]["read_line.start"]();
      return -1;
    },
  });
  try {
    const driver = host.createDriver(guest);
    assert.throws(
      () => driver.step(),
      (failure) => failure === error,
    );
    assert.throws(() => driver.step(), /driver has failed/);
  } finally {
    host.close();
  }
  assert.equal(aborted, 1);
});

test(
  "a compiled conformance guest runs through explicit synchronous turns",
  {
    skip: !process.env.HD_DRIVER_GUEST,
    timeout: 10000,
  },
  async () => {
    const output = [],
      errors = [];
    const module = new WebAssembly.Module(readFileSync(process.env.HD_DRIVER_GUEST));
    const host = createCoreHost(
      {
        out(text) {
          output.push(text);
        },
        err(text) {
          errors.push(text);
        },
      },
      { module },
    );
    try {
      const guest = new WebAssembly.Instance(module, host.imports);
      const driver = host.createDriver(guest);
      for (;;) {
        const turn = driver.step();
        if (turn.status !== -1) {
          assert.equal(turn.status, 0);
          break;
        }
        if (turn.wakes === 0) {
          await delay(1);
        }
      }
      assert.equal(output.join(""), "before\nafter\n");
      assert.deepEqual(errors, []);
      assert.deepEqual(driver.step(), { status: 0, wakes: 0 });
    } finally {
      host.close();
    }
  },
);
