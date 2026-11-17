import assert from "node:assert/strict";
import { test } from "node:test";
import { OperationQueue } from "./operation-queue.ts";
import type { ScheduledOperation } from "./operation-queue.ts";

test("notified completions do linear total work at 1,000 and 10,000 pending calls", () => {
  const measure = (width: number): number => {
    let work = 0;
    const queue = new OperationQueue<ScheduledOperation>();
    const operations = Array.from({ length: width }, (_, handle) => ({
      handle,
      get at() {
        work++;
        return 0;
      },
      get since() {
        work++;
        return handle;
      },
      get promise() {
        work++;
        return false;
      },
      get poll() {
        work++;
        return () => {
          work++;
          return true;
        };
      },
    }));
    for (const operation of operations) {
      queue.add(operation, true);
    }
    // Reverse order rules out relying on registration order for delivery.
    for (let i = width - 1; i >= 0; i--) {
      assert.equal(queue.nextDeadline(), null);
      assert.deepEqual(queue.take(0), []);
      queue.notify(operations[i]);
      queue.notify(operations[i]); // Repeated notification is coalesced.
      assert.deepEqual(queue.take(0), [operations[i]]);
    }
    assert.deepEqual(queue.take(Infinity), []);
    return work;
  };
  const small = measure(1_000);
  const large = measure(10_000);
  assert.equal(large, small * 10);
  assert.ok(large <= 10 * 10_000, `counted queue work: ${large}`);
});

test("ordered deadlines remove cancelled entries and preserve ties", () => {
  const queue = new OperationQueue<ScheduledOperation>();
  const operations = Array.from({ length: 1_000 }, (_, handle) => ({
    handle,
    at: 1_000 - Math.floor(handle / 2),
    since: 1_000 - Math.floor(handle / 2),
    poll: null,
    promise: false,
  }));
  for (const operation of operations) {
    queue.add(operation, false);
  }
  for (const operation of operations) {
    if (operation.handle % 3 === 0) {
      queue.remove(operation);
    }
  }
  const expected = operations
    .filter((operation) => operation.handle % 3 !== 0)
    .sort((left, right) => left.at - right.at || left.handle - right.handle);
  const delivered: ScheduledOperation[] = [];
  while (delivered.length < expected.length) {
    const due = queue.take(Infinity);
    assert.ok(due.length);
    delivered.push(...due);
  }
  assert.equal(queue.nextDeadline(), null);
  assert.deepEqual(delivered, expected);
  assert.deepEqual(queue.take(Infinity), []);
});

test("ready probes are consumed only in their delivery turn", () => {
  const queue = new OperationQueue<ScheduledOperation>();
  let probes = 0;
  const timer = { handle: 1, at: 10, since: 10, poll: null, promise: false };
  const probe = {
    handle: 2,
    at: 20,
    since: 20,
    poll: () => {
      probes++;
      return true;
    },
    promise: false,
  };
  queue.add(timer, false);
  queue.add(probe, true);
  queue.notify(probe);
  assert.deepEqual(queue.take(100), [timer]);
  assert.equal(probes, 0);
  assert.deepEqual(queue.take(100), [probe]);
  assert.equal(probes, 1);
});

test("only legacy probes are swept, including before their retry deadline", () => {
  const queue = new OperationQueue<ScheduledOperation>();
  let ready = false,
    legacyProbes = 0,
    notifiedProbes = 0;
  const legacy = {
    handle: 1,
    at: 0,
    since: 0,
    poll: () => {
      legacyProbes++;
      return ready;
    },
    promise: false,
  };
  const notified = {
    handle: 2,
    at: 0,
    since: 0,
    poll: () => {
      notifiedProbes++;
      return true;
    },
    promise: false,
  };
  queue.add(legacy, false);
  queue.add(notified, true);
  assert.deepEqual(queue.take(0), []);
  assert.equal(legacyProbes, 1);
  ready = true;
  assert.deepEqual(queue.take(0), [legacy]);
  assert.equal(legacyProbes, 2);
  assert.equal(notifiedProbes, 0);
  queue.remove(notified);
  assert.equal(queue.nextDeadline(), null);
});
