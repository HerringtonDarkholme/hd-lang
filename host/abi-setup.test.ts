import assert from "node:assert/strict";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { METHODS } from "./gen/abi.ts";
import { createHost } from "./node.ts";

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
