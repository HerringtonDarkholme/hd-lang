import assert from "node:assert/strict";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { METHODS } from "./gen/abi.ts";
import { createHost } from "./node.ts";
import { createCoreHost } from "./core.ts";
import { Enc } from "./wire.ts";

// Re-export imported functions through real Wasm signatures: scalar
// coercion and discarded void results belong to the Wasm boundary.
function scalarGuest(imports: WebAssembly.Imports): WebAssembly.Exports {
  const types = new Enc().leb(3);
  types.leb(0x60).leb(1).leb(0x7f).leb(1).leb(0x7f);
  types.leb(0x60).leb(1).leb(0x7e).leb(0);
  types.leb(0x60).leb(0).leb(1).leb(0x7e);
  const methods = new Enc().leb(3);
  methods.str("hd:TestRunner").str("row").leb(0).leb(0);
  methods.str("hd:TestRunner").str("report_timeout").leb(0).leb(1);
  methods.str("hd:Clock").str("now").leb(0).leb(2);
  const exports = new Enc().leb(3);
  exports.str("row").leb(0).leb(0);
  exports.str("timeout").leb(0).leb(1);
  exports.str("now").leb(0).leb(2);
  const sections = new Enc()
    .leb(1)
    .bytes(types.done())
    .leb(2)
    .bytes(methods.done())
    .leb(7)
    .bytes(exports.done());
  const module = new WebAssembly.Module(
    new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, ...sections.done()]),
  );
  return new WebAssembly.Instance(module, imports).exports;
}

function codecScans(context: TestContext): () => number {
  let scans = 0;
  const params = new Set<unknown[]>(
    Object.values(METHODS).flatMap((methods) =>
      Object.values(methods).map((message) => message.params),
    ),
  );
  const filter = Array.prototype.filter;
  const descriptor = Object.getOwnPropertyDescriptor(Array.prototype, "filter");
  assert.ok(descriptor);
  context.after(() => Object.defineProperty(Array.prototype, "filter", descriptor));
  Object.defineProperty(Array.prototype, "filter", {
    ...descriptor,
    value: function (this: unknown[], ...args: unknown[]) {
      if (params.has(this)) {
        scans++;
      }
      return Reflect.apply(filter, this, args);
    },
  });
  return () => scans;
}

for (const count of [1000, 10000]) {
  test(`native scalar imports preserve values without adapters at ${count} calls`, (context) => {
    const scans = codecScans(context);
    const draws: bigint[] = [];
    let clock = -(1n << 63n);
    let failure: Error | null = null;
    function row(this: undefined, value: number) {
      assert.equal(this, undefined);
      if (failure) {
        throw failure;
      }
      return value;
    }
    const report_timeout = (value: bigint) => {
      draws.push(value);
      // A void Wasm signature must discard even a non-coercible return.
      return Symbol("ignored");
    };
    const now = () => clock;
    const host = createCoreHost(
      { out() {}, err() {} },
      {},
      {
        providers: () => ({
          traits: { "hd:TestRunner": { row, report_timeout }, "hd:Clock": { now } },
        }),
      },
    );
    context.after(() => host.close());
    assert.equal(host.imports["hd:TestRunner"].row, row);
    assert.equal(host.imports["hd:TestRunner"].report_timeout, report_timeout);
    assert.equal(host.imports["hd:Clock"].now, now);
    const guest = scalarGuest(host.imports);
    const runRow = guest.row as (value: number) => number;
    const timeout = guest.timeout as (value: bigint) => void;
    const runNow = guest.now as () => bigint;
    const bound = scans();
    assert.equal(runNow(), clock);
    clock = (1n << 63n) - 1n;
    assert.equal(runNow(), clock);
    for (const value of [-2147483648, -1, 0, 2147483647]) {
      assert.equal(runRow(value), value);
    }
    for (let i = 0; i < count; i++) {
      assert.equal(runRow(i), i);
      assert.equal(timeout(BigInt(i)), undefined);
    }
    assert.equal(draws.length, count);
    assert.equal(draws[0], 0n);
    assert.equal(draws[count - 1], BigInt(count - 1));
    failure = new Error("scalar provider failed");
    assert.throws(
      () => runRow(0),
      (error) => error === failure,
    );
    assert.equal(scans(), bound);
  });
}

for (const count of [1000, 10000]) {
  test(`unused ABI namespaces build zero codecs across ${count} hosts`, (context) => {
    const scans = codecScans(context);
    for (let i = 0; i < count; i++) {
      const host = createHost({ out() {}, err() {} }, { seed: 1, stdin: null });
      try {
        assert.ok(Object.keys(host.imports).includes("hd:Clock"));
        assert.equal(scans(), i * 3);
        const clock = host.imports["hd:Clock"];
        assert.equal(scans(), (i + 1) * 3);
        assert.equal(host.imports["hd:Clock"], clock);
        assert.equal(scans(), (i + 1) * 3);
        assert.equal(typeof clock.now(), "bigint");
      } finally {
        host.close();
      }
    }
    assert.equal(scans(), count * 3);
  });
}

test("ABI overrides replace unused defaults without building codecs", (context) => {
  const scans = codecScans(context);
  const clock = { now: () => 42n };
  const host = createHost(
    { out() {}, err() {} },
    { seed: 1, stdin: null, providers: () => ({ "hd:Clock": clock }) },
  );
  try {
    assert.equal(scans(), 0);
    assert.equal(host.imports["hd:Clock"], clock);
    assert.equal(host.imports["hd:Clock"].now(), 42n);
    assert.equal(scans(), 0);
    const replacement = { now: () => 43n };
    host.imports["hd:Random"] = replacement;
    assert.equal(host.imports["hd:Random"], replacement);
    assert.equal(scans(), 0);
    assert.ok(Object.keys(host.imports).includes("hd:Random"));
  } finally {
    host.close();
  }
});

test("materialized ABI namespaces remain replaceable and isolated per host", () => {
  const make = () => createHost({ out() {}, err() {} }, { seed: 7, stdin: null });
  const first = make();
  const second = make();
  try {
    assert.notEqual(first.imports["hd:Random"], second.imports["hd:Random"]);
    assert.notEqual(first.core.st, second.core.st);
    const replacement = { now: () => 44n };
    first.imports["hd:Clock"].now();
    first.imports["hd:Clock"] = replacement;
    assert.equal(first.imports["hd:Clock"], replacement);
    assert.notEqual(second.imports["hd:Clock"], replacement);
    assert.equal(delete first.imports["hd:Clock"], true);
  } finally {
    first.close();
    second.close();
  }
});
