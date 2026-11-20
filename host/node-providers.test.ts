import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { createCoreHost } from "./core.ts";
import { createGrant, createHost, nodeLayer } from "./node.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

function allocations(context: TestContext) {
  const NativeMap = Map;
  const NativeBuffer = SharedArrayBuffer;
  let maps = 0;
  const buffers: SharedArrayBuffer[] = [];
  context.after(() => {
    globalThis.Map = NativeMap;
    globalThis.SharedArrayBuffer = NativeBuffer;
  });
  globalThis.Map = new Proxy(NativeMap, {
    construct(target, args, newTarget) {
      maps++;
      return Reflect.construct(target, args, newTarget);
    },
  });
  globalThis.SharedArrayBuffer = new Proxy(NativeBuffer, {
    construct(target, args, newTarget) {
      const buffer = Reflect.construct(target, args, newTarget);
      buffers.push(buffer);
      return buffer;
    },
  });
  return { maps: () => maps, buffers };
}

function platform(st: Dynamic = {}) {
  return nodeLayer.providers({
    env: { stdin: null },
    sink: { out() {}, err() {} },
    st,
    grant: createGrant(),
    flush() {},
    left: () => Infinity,
  });
}

for (const count of [1000, 10000]) {
  test(`unused Node provider lifecycle allocates no client state at ${count} hosts`, (context) => {
    const cost = allocations(context);
    for (let index = 0; index < count; index++) {
      const current = platform();
      Object.keys(current.traits);
      current.drainReady();
      current.leaks();
      current.close();
    }
    assert.equal(cost.maps(), 0);
    assert.equal(cost.buffers.length, 0);
  });
}

test("a Node namespace initializes one platform and caches provider identities", (context) => {
  const cost = allocations(context);
  const current = platform();
  context.after(() => current.close());
  const names = Object.keys(current.traits);
  assert.equal(cost.maps(), 0);
  const sys = current.traits["hd:Sys"];
  const maps = cost.maps();
  const buffers = cost.buffers.length;
  assert.ok(maps > 0);
  assert.equal(buffers, 1);
  for (const name of names) {
    const provider = current.traits[name];
    assert.ok(Object.keys(provider).length > 0, name);
    assert.equal(current.traits[name], provider);
  }
  assert.equal(current.traits["hd:Sys"], sys);
  assert.equal(cost.maps(), maps);
  assert.equal(cost.buffers.length, buffers);
});

test("completion hooks initialize independent reactors without starting clients", (context) => {
  const cost = allocations(context);
  const first: Dynamic = {};
  const second: Dynamic = {};
  const firstPlatform = platform(first);
  const secondPlatform = platform(second);
  context.after(() => {
    firstPlatform.close();
    secondPlatform.close();
  });
  assert.equal(first.completionSyncToken(), 0);
  assert.equal(second.completionSyncToken(), 0);
  assert.equal(cost.maps(), 0);
  assert.equal(cost.buffers.length, 2);
  assert.notEqual(cost.buffers[0], cost.buffers[1]);
  assert.equal(first.waitCompletionSync(0, 0), "timed-out");
  assert.ok(firstPlatform.traits["hd:Sys"]);
  assert.ok(secondPlatform.traits["hd:Sys"]);
  assert.equal(cost.buffers.length, 2);
});

test("lazy Node hosts retain independent temporary directories and cleanup", (context) => {
  const first = createHost({ out() {}, err() {} }, { stdin: null });
  const second = createHost({ out() {}, err() {} }, { stdin: null });
  context.after(() => {
    first.close();
    second.close();
  });
  const firstDirectory = first.tempDir?.();
  const secondDirectory = second.tempDir?.();
  assert.ok(typeof firstDirectory === "string");
  assert.ok(typeof secondDirectory === "string");
  assert.notEqual(firstDirectory, secondDirectory);
  assert.equal(first.tempDir?.(), firstDirectory);
  assert.equal(second.tempDir?.(), secondDirectory);
  assert.ok(existsSync(firstDirectory));
  assert.ok(existsSync(secondDirectory));
  first.close();
  assert.equal(existsSync(firstDirectory), false);
  assert.ok(existsSync(secondDirectory));
  second.close();
  assert.equal(existsSync(secondDirectory), false);
});

test("a lazy platform merges its partial provider with the core provider", (context) => {
  let initialized = 0;
  const traits = Object.defineProperty({}, "hd:TestRunner", {
    enumerable: true,
    get() {
      initialized++;
      return { temp_dir: () => "/tmp" };
    },
  });
  const host = createCoreHost(
    { out() {}, err() {} },
    { runner: { row: () => 2 } },
    {
      providers: () => ({ traits }),
    },
  );
  context.after(() => host.close());
  assert.equal(initialized, 0);
  Object.keys(host.imports);
  assert.equal(initialized, 0);
  const runner = host.imports["hd:TestRunner"];
  assert.equal(initialized, 1);
  assert.equal(runner.row(3), 2);
  assert.equal(host.imports["hd:TestRunner"], runner);
  assert.equal(initialized, 1);
});

test("explicit namespace replacement skips a lazy platform default", (context) => {
  let initialized = 0;
  const traits = Object.defineProperty({}, "hd:Clock", {
    enumerable: true,
    get() {
      initialized++;
      throw new Error("unused platform default");
    },
  });
  const replacement = { marker: true };
  const host = createCoreHost(
    { out() {}, err() {} },
    {
      providers: () => ({ "hd:Clock": replacement }),
    },
    { providers: () => ({ traits }) },
  );
  context.after(() => host.close());
  assert.equal(host.imports["hd:Clock"], replacement);
  assert.equal(initialized, 0);
});
