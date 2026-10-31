import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";
import { test } from "node:test";
import { createLeaseDispatcher } from "../adapters/dispatcher.ts";

const context = {
  pending: (poll, done, abort = () => {}, finish = () => {}) => ({ poll, done, abort, finish }),
};
const response = (text) => ({
  status: 200,
  headers: [],
  body: new TextEncoder().encode(text),
  url: "",
});
const tick = () => new Promise((resolve) => setImmediate(resolve));

test(
  "a poll by B queues A's next operation and disposal for A's owner",
  { timeout: 5000 },
  async () => {
    const owner = new AsyncLocalStorage();
    const events = [];
    let left,
      right,
      first,
      second,
      completeFirst,
      completeSecond,
      phase = 0;
    const operation = (name, complete) => (resolve) => {
      events.push([name, owner.getStore()]);
      complete(resolve);
      return () => events.push(["dispose " + name, owner.getStore()]);
    };
    const dispatcher = createLeaseDispatcher({
      drive() {
        if (phase === 0) {
          phase = 1;
          first = left.operation(
            context,
            operation("first", (resolve) => {
              completeFirst = resolve;
            }),
          );
        } else if (phase === 1 && first.poll()) {
          assert.equal(first.done(), "one");
          first.finish();
          phase = 2;
          second = left.operation(
            context,
            operation("second", (resolve) => {
              completeSecond = resolve;
            }),
          );
          right.respond(response("B finished"));
          right.finish();
        } else if (phase === 2 && second.poll()) {
          assert.equal(second.done(), "two");
          second.finish();
          left.respond(response("A finished"));
          left.finish();
          phase = 3;
        }
      },
    });
    left = dispatcher.open("A");
    right = dispatcher.open("B");
    const resultA = owner.run("A", () => left.run());
    const resultB = owner.run("B", () => right.run());
    // A's completion is made ready in B's turn, the adversarial ordering.
    owner.run("B", () => completeFirst("one"));
    await resultB;
    await tick();
    owner.run("A", () => completeSecond("two"));
    assert.equal(new TextDecoder().decode((await resultA).response.body), "A finished");
    assert.deepEqual(events, [
      ["first", "A"],
      ["dispose first", "A"],
      ["second", "A"],
      ["dispose second", "A"],
    ]);
    assert.equal(dispatcher.active(), 0);
  },
);

test("abort before start drops I/O; late completions cannot wake a later lease", async () => {
  let turns = 0,
    starts = 0,
    resolveOld;
  const dispatcher = createLeaseDispatcher({
    drive() {
      turns++;
    },
  });
  const lease = dispatcher.open("A");
  const dropped = lease.operation(context, () => {
    starts++;
  });
  dropped.abort();
  lease.operation(context, (resolve) => {
    starts++;
    resolveOld = resolve;
  });
  const running = lease.run();
  assert.equal(starts, 1);
  lease.finish(true);
  await running;
  const next = dispatcher.open("B");
  assert.notEqual(next.id.sequence, lease.id.sequence);
  const before = turns;
  resolveOld("too late");
  assert.equal(turns, before);
  assert.throws(() => lease.operation(context, () => {}), /expired request lease/);
  next.finish();
  await next.run();
  assert.equal(dispatcher.active(), 0);
});

test("response publication waits for cleanup acknowledgement and copies guest data", async () => {
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const lease = dispatcher.open("A");
  const value = response("original");
  let settled = false;
  const running = lease.run().then((result) => {
    settled = true;
    return result;
  });
  lease.respond(value);
  value.body.fill(0);
  value.headers.push(["late", "mutation"]);
  await tick();
  assert.equal(settled, false);
  lease.finish();
  const result = await running;
  assert.equal(new TextDecoder().decode(result.response.body), "original");
  assert.deepEqual(result.response.headers, []);
});

test("a trap drains native aborts in every owner before failing their requests", async () => {
  const owner = new AsyncLocalStorage();
  const disposed = [];
  let failed;
  const dispatcher = createLeaseDispatcher({
    drive() {},
    failed(error) {
      failed = error;
    },
  });
  const firstLease = dispatcher.open("A"),
    secondLease = dispatcher.open("B");
  for (const lease of [firstLease, secondLease]) {
    lease.operation(context, () => () => disposed.push(owner.getStore()));
  }
  const ra = owner.run("A", () => firstLease.run());
  const rb = owner.run("B", () => secondLease.run());
  const trap = new Error("private trap detail");
  owner.run("B", () => dispatcher.fail(trap));
  const results = await Promise.allSettled([ra, rb]);
  assert.ok(results.every((result) => result.status === "rejected" && result.reason === trap));
  assert.equal(failed, trap);
  assert.deepEqual(disposed.sort(), ["A", "B"]);
  assert.equal(dispatcher.active(), 0);
  assert.throws(() => dispatcher.open("C"), /server instance failed/);
});

test("cancellation is local, exactly once, and cleanup disposal stays with its owner", async () => {
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const firstLease = dispatcher.open("A"),
    secondLease = dispatcher.open("B");
  const cancelled = firstLease.cancelled(context);
  firstLease.cancel("Disconnected");
  firstLease.cancel("Deadline");
  assert.ok(cancelled.poll());
  assert.equal(cancelled.done().tag, "Disconnected");
  assert.equal(secondLease.cancelled(context).poll(), false);
  assert.equal(firstLease.respond(response("too late")), false);
  firstLease.finish(true);
  secondLease.respond(response("fine"));
  secondLease.finish();
  const [ra, rb] = await Promise.all([firstLease.run(), secondLease.run()]);
  assert.equal(ra.cancelled, "Disconnected");
  assert.equal(ra.abandoned, true);
  assert.equal(rb.response.status, 200);
});

test("ready then aborted operations dispose once without delivering their value", async () => {
  let ready,
    disposed = 0;
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const lease = dispatcher.open("A");
  const operation = lease.operation(context, (resolve) => {
    ready = resolve;
    return () => {
      disposed++;
    };
  });
  const running = lease.run();
  ready("owned value");
  assert.ok(operation.poll());
  operation.abort();
  operation.abort();
  lease.finish(true);
  await running;
  assert.equal(disposed, 1);
  ready("late duplicate");
  assert.equal(disposed, 1);
});

test("a failing shutdown hook cannot bypass queued native disposal", async () => {
  let disposed = false;
  const dispatcher = createLeaseDispatcher({
    drive() {},
    failed() {
      throw new Error("shutdown hook failed too");
    },
  });
  const lease = dispatcher.open("A");
  lease.operation(context, () => () => {
    disposed = true;
  });
  const running = lease.run();
  const fault = new Error("original guest fault");
  dispatcher.fail(fault);
  await assert.rejects(running, (error) => error === fault);
  assert.equal(disposed, true);
  assert.equal(dispatcher.active(), 0);
});
