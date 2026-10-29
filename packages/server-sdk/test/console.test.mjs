import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";
import { test } from "node:test";
import { createRequestConsole } from "../adapters/console.mjs";
import { createLeaseDispatcher } from "../adapters/dispatcher.mjs";

test("Console captures immutable identity and delivers in its owner after another poll", async () => {
  const owner = new AsyncLocalStorage();
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const lease = dispatcher.open("A"),
    records = [],
    id = { generation: 91n, sequence: 42n };
  const console = createRequestConsole(lease, {
    id,
    policy: { allows: () => true },
    write(record) {
      records.push([owner.getStore(), record]);
    },
  });
  id.sequence = 999n;
  owner.run("B's guest turn", () => {
    assert.equal(console.write_sync("first\n").ok, true);
    assert.equal(console.write_error_sync("λ\n").ok, true);
  });
  assert.deepEqual(records, [], "capture itself performs no native logging effect");
  lease.cancel("Disconnected");
  assert.equal(console.write_sync("cleanup\n").ok, true);
  lease.finish(false);
  await owner.run("A", () => lease.run());
  assert.deepEqual(
    records.map(([at, record]) => [at, record.lease, record.stream, record.text]),
    [
      ["A", { generation: "91", sequence: "42" }, "out", "first\n"],
      ["A", { generation: "91", sequence: "42" }, "err", "λ\n"],
      ["A", { generation: "91", sequence: "42" }, "out", "cleanup\n"],
    ],
  );
  assert.ok(records.every(([, record]) => record.version === 1 && Object.isFrozen(record)));
  assert.ok(records.every(([, record]) => Object.isFrozen(record.lease)));
  assert.throws(() => console.write_sync("late"), /expired request lease/);
  assert.throws(() => console.write_error_sync("late"), /expired request lease/);
  assert.equal(dispatcher.active(), 0);
});

test("denied Console captures no record and returns each stream's NotGranted", async () => {
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const lease = dispatcher.open("denied"),
    records = [];
  const console = createRequestConsole(lease, {
    policy: { allows: () => false },
    write: (record) => records.push(record),
  });
  for (const [method, stream] of [
    ["write_sync", "Stdout"],
    ["write_error_sync", "Stderr"],
  ])
    assert.deepEqual(console[method]("private"), {
      ok: false,
      error: { tag: "NotGranted", fields: { stream: { tag: stream, fields: {} } } },
    });
  lease.finish(false);
  await lease.run();
  assert.deepEqual(records, []);
});

test("a throwing log observer does not prevent later delivery or lease completion", async () => {
  const dispatcher = createLeaseDispatcher({ drive() {} });
  const lease = dispatcher.open("observer"),
    delivered = [];
  const console = createRequestConsole(lease, {
    policy: { allows: () => true },
    write(record) {
      delivered.push(record.text);
      if (record.stream === "err") throw new Error("observer failed");
    },
  });
  assert.throws(() => lease.effect(null), /must be a function/);
  assert.throws(() => console.write_sync({ secret: "not text" }), /needs text/);
  console.write_error_sync("first");
  console.write_sync("second");
  lease.finish(true);
  assert.equal((await lease.run()).abandoned, true);
  assert.deepEqual(delivered, ["first", "second"]);
});
