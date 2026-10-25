import assert from "node:assert/strict";
import { test } from "node:test";
import http from "node:http";
import { once } from "node:events";
import { createSupervisor } from "../adapters/supervisor.mjs";

const module = new WebAssembly.Module(
  new Uint8Array([
    0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 7, 8, 1, 4, 115, 112, 105, 110, 0,
    0, 10, 9, 1, 7, 0, 3, 64, 12, 0, 11, 11,
  ]),
);
// Test-only budgets. None is a proposed product default.
const testPolicy = Object.freeze({
  workers: 2,
  startupMs: 1500,
  requestMs: 250,
  drainMs: 250,
  stopKillMs: 300,
  restartLimit: 1,
  restartWindowMs: 10000,
  retryAfter: "1",
});
function harness(policy = {}, fixture, log = () => {}) {
  const events = [],
    watchers = [];
  const supervisor = createSupervisor({
    program: Object.freeze({
      module,
      runtimeURL: new URL("./fixtures/supervisor/runtime.mjs", import.meta.url).href,
      bindingsURL: new URL("./fixtures/supervisor/bindings.mjs", import.meta.url).href,
      applicationURL: new URL("./fixtures/supervisor/application.mjs", import.meta.url).href,
      grants: Object.freeze({ fixture }),
    }),
    policy: { ...testPolicy, ...policy },
    log,
    event(event) {
      events.push(event);
      // Matching watchers remove themselves; iterate a stable snapshot.
      for (const watch of watchers.slice())
        if (watch.predicate(event)) {
          watchers.splice(watchers.indexOf(watch), 1);
          watch.resolve(event);
        }
    },
  });
  return {
    supervisor,
    events,
    wait(predicate) {
      const found = events.find(predicate);
      return found
        ? Promise.resolve(found)
        : new Promise((resolve) => watchers.push({ predicate, resolve }));
    },
    async ready() {
      await Promise.all(
        Array.from({ length: policy.workers ?? 2 }, (_, slot) =>
          this.wait((e) => e.kind === "ready" && e.slot === slot),
        ),
      );
    },
  };
}
const request = (url, delay) => ({ url, delay });
const body = (result) => new TextDecoder().decode(result.response.body);

test("an outer stop cannot shorten the fixed cleanup backstop", () => {
  assert.throws(() => harness({ stopKillMs: 199 }), /cannot precede the cleanup backstop/);
});

test(
  "one active request per worker, no queue, retained state and owned copied results",
  { timeout: 10000 },
  async () => {
    const f = harness({ requestMs: 3000 });
    try {
      await f.ready();
      assert.equal(f.supervisor.ready(), true);
      const a = f.supervisor.dispatch(request("/ok", 20)),
        b = f.supervisor.dispatch(request("/ok", 20));
      const overload = await f.supervisor.dispatch(request("/never"));
      assert.equal(overload.response.status, 503);
      assert.deepEqual(overload.response.headers, [["Retry-After", "1"]]);
      assert.deepEqual((await Promise.all([a, b])).map(body), ["1", "1"]);
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "2");
      await f.supervisor.stop();
      assert.equal(f.supervisor.state(), "Stopped");
      assert.equal(f.events.filter((e) => e.kind === "drained").length, 2);
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "trap poisons only its instance, replies generic500 and starts a fresh generation",
  { timeout: 10000 },
  async () => {
    const f = harness({ requestMs: 3000 });
    try {
      await f.ready();
      const failed = f.supervisor.dispatch(request("/trap")),
        sibling = f.supervisor.dispatch(request("/ok", 30));
      const result = await failed;
      assert.equal(result.response.status, 500);
      assert.equal(body(result), "");
      assert.equal(body(await sibling), "1");
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "1");
      assert.equal(JSON.stringify(f.events).includes("private-error"), false);
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "CPU-bound Wasm handler cannot prevent outer deadline or sibling response",
  { timeout: 10000 },
  async () => {
    const f = harness();
    try {
      await f.ready();
      const loop = f.supervisor.dispatch(request("/cpu")),
        sibling = f.supervisor.dispatch(request("/ok", 15));
      assert.equal(body(await sibling), "1");
      const result = await loop;
      assert.equal(result.cancelled, "Deadline");
      assert.equal(result.abandoned, true);
      assert.equal(
        f.events.some((e) => e.kind === "terminated" && e.cause === "cleanup-deadline"),
        true,
      );
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "1");
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "CPU-bound main is never ready and bounded restart exhaustion is reported",
  { timeout: 10000 },
  async () => {
    const f = harness({ workers: 1, startupMs: 180, restartLimit: 1 }, "main-cpu");
    try {
      assert.equal((await f.supervisor.dispatch(request("/not-admitted"))).response.status, 503);
      await f.wait((e) => e.kind === "restart-exhausted");
      assert.equal(f.supervisor.ready(), false);
      assert.equal(f.supervisor.active(), 0);
      assert.equal(f.events.filter((e) => e.kind === "starting").length, 2);
      assert.equal(f.events.filter((e) => e.kind === "ready").length, 0);
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "disconnect cancels once; nonreturning cleanup is killed without affecting sibling",
  { timeout: 10000 },
  async () => {
    const f = harness({ requestMs: 3000 });
    try {
      await f.ready();
      const signal = new AbortController();
      const a = f.supervisor.dispatch(request("/cleanup-cpu"), signal.signal);
      // Cross-thread FIFO guarantees request precedes cancellation, even before its handler yields.
      signal.abort();
      signal.abort();
      const b = f.supervisor.dispatch(request("/ok"));
      assert.equal(body(await b), "1");
      const result = await a;
      assert.equal(result.cancelled, "Disconnected");
      assert.equal(result.abandoned, true);
      assert.equal(
        f.events.filter((e) => e.kind === "cancel" && e.reason === "Disconnected").length,
        1,
      );
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "1");
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "first stop drains without cancellation; second stop escalates and joins termination",
  { timeout: 10000 },
  async () => {
    const f = harness({ requestMs: 3000, drainMs: 3000 });
    try {
      await f.ready();
      const completed = f.supervisor.dispatch(request("/ok", 30));
      const pending = f.supervisor.dispatch(request("/wait"));
      const draining = f.supervisor.stop();
      assert.equal(f.supervisor.ready(), false);
      assert.equal((await f.supervisor.dispatch(request("/not-admitted"))).response.status, 503);
      assert.equal(body(await completed), "1");
      assert.equal(f.events.filter((e) => e.kind === "cancel").length, 0);
      const escalated = f.supervisor.stop();
      assert.equal((await pending).cancelled, "Stopping");
      await Promise.all([draining, escalated]);
      assert.equal(f.supervisor.state(), "Stopped");
      assert.equal(f.supervisor.active(), 0);
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "wrong-generation and out-of-order replies cannot release a later physical response",
  { timeout: 10000 },
  async () => {
    const f = harness({ workers: 1, requestMs: 3000 });
    try {
      await f.ready();
      assert.equal(body(await f.supervisor.dispatch(request("/stale-messages"))), "1");
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "2");
      assert.equal(f.events.filter((e) => e.kind === "terminated").length, 0);
      assert.equal(f.events.filter((e) => e.kind === "released").length, 2);
    } finally {
      await f.supervisor.close();
    }
  },
);

test(
  "log observers reject stale identities and cannot poison response ownership",
  {
    timeout: 10000,
  },
  async () => {
    const records = [];
    const f = harness({ workers: 1, requestMs: 3000 }, undefined, (record) => {
      records.push(record);
      throw new Error("private observer failed");
    });
    try {
      await f.ready();
      assert.equal(body(await f.supervisor.dispatch(request("/log-messages"))), "1");
      assert.equal(body(await f.supervisor.dispatch(request("/ok"))), "2");
      assert.equal(records.length, 1);
      assert.equal(records[0].text, "valid log");
      assert.equal(records[0].request, 1);
      assert.equal(records[0].headers, undefined);
      assert.deepEqual(records[0].lease, { generation: "1", sequence: "1" });
      assert.ok(Object.isFrozen(records[0].lease));
      assert.deepEqual(records[0].worker, { slot: 0, generation: 1 });
      assert.ok(Object.isFrozen(records[0]) && Object.isFrozen(records[0].worker));
      assert.equal(f.events.filter((e) => e.kind === "terminated").length, 0);
    } finally {
      await f.supervisor.close();
    }
  },
);

test("body-reading time consumes the original admission budget", { timeout: 10000 }, async () => {
  const f = harness({ workers: 1, requestMs: 1000 });
  try {
    await f.ready();
    const result = await f.supervisor.dispatch(request("/wait"), undefined, {
      admittedAt: performance.now() - 1001,
    });
    assert.equal(result.cancelled, "Deadline");
    assert.equal(result.abandoned, false);
    assert.equal(f.events.filter((e) => e.kind === "deadline-before-dispatch").length, 1);
    assert.equal(f.events.filter((e) => e.kind === "admitted").length, 0);
  } finally {
    await f.supervisor.close();
  }
});

// Test scaffold only: Fetch/Node adapters own production body/header mapping.
async function physicalListener(f) {
  const outcomes = [];
  const server = http.createServer(async (request, response) => {
    const controller = new AbortController();
    response.once("close", () => {
      if (!response.writableFinished) controller.abort("Disconnected");
    });
    const url = new URL(request.url, "http://listener.invalid");
    const result = await f.supervisor.dispatch(
      { url: url.pathname, delay: Number(url.searchParams.get("delay")) },
      controller.signal,
    );
    outcomes.push(result);
    if (response.destroyed) return;
    const value = result.response ?? {
      status: result.cancelled === "Deadline" ? 504 : 503,
      headers: [],
      body: new Uint8Array(),
    };
    for (const [key, text] of value.headers) response.setHeader(key, text);
    response.writeHead(value.status);
    response.end(value.body);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    outcomes,
    url: `http://127.0.0.1:${server.address().port}`,
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

test(
  "physical responses remain outside the hung isolate with concurrent/disconnected requests",
  { timeout: 10000 },
  async () => {
    const f = harness();
    let listener;
    try {
      await f.ready();
      listener = await physicalListener(f);
      const cpu = fetch(listener.url + "/cpu");
      await f.wait((e) => e.kind === "admitted" && e.id === 1);
      const disconnected = new AbortController();
      const pending = fetch(listener.url + "/cleanup-cpu", { signal: disconnected.signal });
      pending.catch(() => {});
      await f.wait((e) => e.kind === "admitted" && e.id === 2);
      const overloaded = await fetch(listener.url + "/never");
      assert.equal(overloaded.status, 503);
      assert.equal(overloaded.headers.get("Retry-After"), "1");
      await overloaded.arrayBuffer();
      disconnected.abort();
      const timedOut = await cpu;
      assert.equal(timedOut.status, 504);
      await timedOut.arrayBuffer();
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      const later = await fetch(listener.url + "/ok");
      assert.equal(later.status, 200);
      await later.arrayBuffer();
      assert.equal(
        listener.outcomes.some((result) => result.cancelled === "Disconnected" && result.abandoned),
        true,
      );
      assert.equal(
        listener.outcomes.some((result) => result.cancelled === "Deadline" && result.abandoned),
        true,
      );
    } finally {
      await f.supervisor.close();
      if (listener) await listener.close();
    }
  },
);
