import assert from "node:assert/strict";
import { test } from "node:test";
import { createCoreHost, pendingOperation, HostContract } from "./core.ts";
import { Enc, Ok } from "./wire.ts";
import { readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";
import { PROGRAM_TIME_LIMIT, reportIn } from "./run-support.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// A real module forwards its root ABI exports to observable callbacks.
// Completion and exchange-memory handling remain the production core's.
function instance(callbacks: Dynamic, resultType = 0x7f): Dynamic {
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

for (const delegated of [false, true]) {
  for (const fact of [null, "deadline_fact"]) {
    test(`a program deadline ${delegated ? "delegates" : "writes"} its report (fact ${fact})`, async () => {
      const errors: string[] = [],
        reports: string[] = [];
      const host = createCoreHost(
        {
          out() {},
          err: (text) => errors.push(text),
          ...(delegated ? { programTimeLimit: (text: string) => reports.push(text) } : {}),
        },
        { deadline: performance.timeOrigin + performance.now() - 1 },
      );
      try {
        const guest = instance({
          poll() {
            if (fact !== null) {
              host.core.st.facts.push(fact);
            }
            return 0;
          },
        });
        const result = await host.run(guest, "hd.init", "hd.poll");
        assert.equal(result.status, 3);
        const expected = [reportIn(fact, PROGRAM_TIME_LIMIT)];
        assert.deepEqual(errors, delegated ? [] : expected);
        assert.deepEqual(reports, delegated ? expected : []);
      } finally {
        host.close();
      }
    });
  }
}

test("a case deadline stays with its case reporter", async () => {
  const errors: string[] = [];
  const host = createCoreHost(
    {
      out() {},
      err: (text) => errors.push(text),
      programTimeLimit() {
        assert.fail("a case timeout is not a program timeout");
      },
    },
    { deadline: performance.timeOrigin + performance.now() - 1 },
  );
  try {
    const guest = instance({
      poll() {
        host.core.st.timeoutMs = 7;
        return 0;
      },
    });
    assert.equal((await host.run(guest, "hd.init", "hd.poll")).status, 3);
    assert.deepEqual(errors, [
      "panic: time-limit: the test case ran longer than its timeout of 7ms\n",
    ]);
  } finally {
    host.close();
  }
});

test("a step initializes once, publishes wakes, and retains its terminal status", () => {
  const host = createCoreHost({ out() {}, err() {} });
  const events: string[] = [];
  let handle: number | undefined,
    ready = false,
    first = true;
  const guest = instance({
    init() {
      events.push("init");
    },
    poll(context: number) {
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
    wake(count: number) {
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

for (const mode of ["step", "async", "block"]) {
  test(`overdue timers retain deadline order and coalesce ties (${mode})`, async (context) => {
    let now = 100;
    context.mock.method(performance, "now", () => now);
    const host = createCoreHost({ out() {}, err() {} });
    let handles: number[] | null = null;
    const batches: number[][] = [];
    const wake = (count: number) => {
      const words = new DataView(guest.exports["hd.x"].buffer);
      const batch = Array.from({ length: count }, (_, i) => words.getUint32(i * 4, true));
      batches.push(batch);
      for (const handle of batch) {
        host.imports["hd:Clock"]["sleep.finish"](handle);
      }
    };
    const guest = instance({
      wake,
      poll() {
        if (handles === null) {
          // Work is polled first, but its timer is later than the timeout.
          handles = [20n, 1n, 1n].map((ms) => host.imports["hd:Clock"]["sleep.start"](ms)[1]);
          // The driver thread resumes after all three deadlines, as under load.
          now = 125;
          if (mode === "block") {
            wake(host.imports["hd:rt"].block());
            wake(host.imports["hd:rt"].block());
            return 0;
          }
          return -1;
        }
        return batches.length === 2 ? 0 : -1;
      },
    });
    try {
      if (mode === "step") {
        const driver = host.createDriver(guest);
        assert.deepEqual(driver.step(), { status: -1, wakes: 2 });
        assert.deepEqual(driver.step(), { status: -1, wakes: 1 });
        assert.deepEqual(driver.step(), { status: 0, wakes: 0 });
      } else {
        assert.equal((await host.run(guest, "hd.init", "hd.poll")).status, 0);
      }
      assert.ok(handles);
      assert.deepEqual(batches, [[handles[1], handles[2]], [handles[0]]]);
    } finally {
      host.close();
    }
  });
}

test("idle turns retain pollable I/O without creating a timer or background driver", async () => {
  let ready = false,
    woken = false,
    handle: number | undefined,
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
    globalThis.setTimeout = (() => {
      throw new Error("a step must not create a timer");
    }) as Dynamic;
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

test("an async driver retains synchronous probe waits without a JSPI stack", async () => {
  const errors: string[] = [];
  const host = createCoreHost({ out() {}, err: (text) => errors.push(text) });
  try {
    const guest = instance({
      poll() {
        host.imports["hd:Clock"]["sleep.start"](1n);
        host.imports["hd:rt"].block();
        return 0;
      },
    });
    const result = await host.run(guest, "hd.init", "hd.poll");
    assert.equal(result.status, 0);
    assert.equal(result.panic, null);
    assert.deepEqual(errors, []);
  } finally {
    host.close();
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
  let driver: Dynamic;
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
  const output: string[] = [];
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
    const output: string[] = [],
      errors: string[] = [];
    const module = new WebAssembly.Module(
      new Uint8Array(readFileSync(process.env.HD_DRIVER_GUEST!)),
    );
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
