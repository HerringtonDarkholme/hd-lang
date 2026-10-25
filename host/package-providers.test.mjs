import assert from "node:assert/strict";
import { test } from "node:test";
import { createPackageRuntime, readPackageContract } from "./package-providers.mjs";
import { Dec, Enc, HostContract } from "./wire.mjs";
import { createCoreHost, pendingOperation, isAsciiUrl, writtenHost } from "./core.mjs";

const runtime = () => createPackageRuntime({});
const encode = (codec, value) => {
  const e = new Enc();
  codec.enc(e, value, []);
  return e.done();
};
const roundTrip = (codec, value) => {
  const d = new Dec(encode(codec, value));
  const result = codec.dec(d);
  assert.equal(d.at, d.b.length);
  return result;
};

test("package codecs retain exact integer ranges, float widths and void", () => {
  const r = runtime();
  for (const bits of [8, 16, 32, 64]) {
    for (const signed of [false, true]) {
      const t = r.int(bits, signed);
      const low = signed ? -(1n << BigInt(bits - 1)) : 0n;
      const high = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
      for (const n of [low, 0n, high]) assert.equal(roundTrip(t, n), bits === 64 ? n : Number(n));
      for (const bad of [low - 1n, high + 1n, 1.1, NaN, Infinity, "1", null])
        assert.throws(() => encode(t, bad), HostContract);
    }
  }
  assert.equal(encode(r.f32, 1.25).length, 4);
  assert.equal(encode(r.f64, 1.25).length, 8);
  assert.equal(roundTrip(r.f32, 1 / 3), Math.fround(1 / 3));
  assert.ok(Object.is(roundTrip(r.f64, -0), -0));
  assert.ok(Number.isNaN(roundTrip(r.f32, NaN)));
  assert.equal(roundTrip(r.unit, undefined), undefined);
  assert.throws(() => encode(r.unit, null), HostContract);
  assert.throws(() => r.f64.dec(new Dec(new Uint8Array(7))), HostContract);
  assert.throws(() => encode(r.bool, 1), HostContract);
  assert.equal(roundTrip(r.bool, false), false);
});

test("package codecs own their data and preserve names including __proto__", () => {
  const r = runtime();
  const record = r.record([
    ["__proto__", r.str],
    ["number", r.int(64, false)],
  ]);
  const input = Object.fromEntries([
    ["__proto__", "literal data"],
    ["number", 18446744073709551615n],
  ]);
  const result = roundTrip(record, input);
  assert.deepEqual(result, input);
  assert.equal(Object.getPrototypeOf(result), Object.prototype);
  assert.throws(() => encode(record, { number: 1n }), HostContract);
  assert.deepEqual(roundTrip(r.bytes, new Uint8Array([0, 255])), new Uint8Array([0, 255]));
  assert.throws(() => encode(r.bytes, [1]), HostContract);
  assert.deepEqual(roundTrip(r.tuple([r.char, r.option(r.str)]), ["🦉", null]), ["🦉", null]);
  assert.throws(() => encode(r.char, "two"), HostContract);
  assert.throws(() => encode(r.char, "\ud800"), HostContract);
  assert.deepEqual(roundTrip(r.list(r.unit), [undefined, undefined]), [undefined, undefined]);
  const enumeration = r.enumeration([
    ["Empty", []],
    ["Broken", [["reason", r.str]]],
  ]);
  assert.deepEqual(roundTrip(enumeration, { tag: "Broken", fields: { reason: "why" } }), {
    tag: "Broken",
    fields: { reason: "why" },
  });
  assert.throws(() => encode(enumeration, { tag: "Other" }), HostContract);
  const res = r.result(r.str, enumeration);
  assert.deepEqual(roundTrip(res, { ok: false, error: { tag: "Empty", fields: {} } }), {
    ok: false,
    error: { tag: "Empty", fields: {} },
  });
  const withTag = r.enumeration([["Event", [["tag", r.int(32, false)]]]]);
  assert.deepEqual(roundTrip(withTag, { tag: "Event", fields: { tag: 7 } }), {
    tag: "Event",
    fields: { tag: 7 },
  });
  assert.throws(() => encode(res, { ok: 0, error: {} }), HostContract);
  const privateValue = {
    large: 18446744073709551615n,
    signed: -9223372036854775808n,
    float: 1.0,
    nested: [null, false, "🦉", { empty: [] }],
    __proto__: null,
  };
  assert.deepEqual(roundTrip(r.json, privateValue), { ...privateValue });
  assert.equal(
    roundTrip(r.json, "a custom string representation"),
    "a custom string representation",
  );
  assert.ok(Object.is(roundTrip(r.json, -0), -0));
});

test("package contract reader rejects truncated, noncanonical and unknown versions", () => {
  const empty = new Uint8Array([1, 0, 0, 0, 0]);
  assert.deepEqual(readPackageContract(empty), []);
  for (let n = 0; n < empty.length; n++)
    assert.throws(() => readPackageContract(empty.subarray(0, n)), HostContract);
  for (const bytes of [
    [2, 0, 0, 0, 0],
    [1, 0, 0, 0, 128, 0],
    [1, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 255, 255, 255, 255, 15],
  ])
    assert.throws(() => readPackageContract(new Uint8Array(bytes)), HostContract);
});

test("private Serde values retain numeric kind, deep containers and literal object keys", () => {
  const r = runtime();
  const input = Object.fromEntries([
    ["__proto__", { count: 9007199254740993n }],
    ["f", 1.0],
  ]);
  assert.deepEqual(roundTrip(r.json, input), input);
  const wire = (tokens) => {
    const e = new Enc();
    e.str(JSON.stringify(tokens));
    return new Dec(e.done());
  };
  assert.equal(r.json.dec(wire([["f", "3ff0000000000000"]])), 1);
  assert.equal(r.json.dec(wire([["u", "1"]])), 1n);
  for (const tokens of [
    [],
    [["u", "18446744073709551616"]],
    [["f", "7ff0000000000000"]],
    [["i", "-9223372036854775809"]],
    [["a", 2], ["n"]],
    [["o", 1], ["b", true], ["n"]],
    [["n"], ["n"]],
  ])
    assert.throws(() => r.json.dec(wire(tokens)), HostContract);
  for (const bad of [undefined, () => {}, 1n << 64n, -(1n << 63n) - 1n, new Date(), [undefined]])
    assert.throws(() => encode(r.json, bad), HostContract);
  const cyclic = {};
  cyclic.self = cyclic;
  assert.throws(() => encode(r.json, cyclic), /cyclic/);
  const shared = { value: 7n };
  assert.deepEqual(roundTrip(r.json, [shared, shared]), [shared, shared]);
  let deep = null;
  for (let i = 0; i < 128; ++i) deep = [deep];
  let decoded = roundTrip(r.json, deep);
  for (let i = 0; i < 128; ++i) {
    assert.equal(decoded.length, 1);
    decoded = decoded[0];
  }
  assert.equal(decoded, null);
});

// A tiny module with the real contract custom section and exchange memory.
// Its methods are driven directly so readiness and delivery can be separated.
function operationHost(wait, suspends = true, grants = {}, extraEnv = {}) {
  const identity = { package: "tests/sdk", version: "1.0.0", trait_path: "Source" };
  const moduleName = "hd:pkg:readiness-test";
  const e = new Enc();
  e.leb(1);
  e.str(identity.package).str(identity.version).str(identity.trait_path).str(moduleName);
  e.leb(1).str("pkg.Source").leb(0).leb(1);
  e.str("wait")
    .str("wait")
    .leb(suspends ? 1 : 0)
    .leb(0)
    .leb(6);
  const contract = new Uint8Array([1, 0, 0, 0, ...e.done()]);
  const custom = new Uint8Array([...new Enc().str("hd.capabilities").done(), ...contract]);
  const memory = new Enc().leb(1).leb(0).leb(1).done();
  const exported = new Enc().leb(1).str("hd.x").leb(2).leb(0).done();
  const bytes = new Uint8Array([
    0,
    97,
    115,
    109,
    1,
    0,
    0,
    0,
    ...new Enc().leb(0).bytes(custom).leb(5).bytes(memory).leb(7).bytes(exported).done(),
  ]);
  const module = new WebAssembly.Module(bytes);
  const host = createCoreHost(
    { out() {}, err() {} },
    {
      ...extraEnv,
      module,
      grants: { "pkg.Source": true, ...grants },
      packageBindings: (runtime) =>
        runtime.bind(
          contract,
          [
            {
              module: moduleName,
              handle: false,
              methods: [
                {
                  name: "wait",
                  member: "wait",
                  wait: suspends ? "may" : "never",
                  params: [],
                  result: runtime.str,
                },
              ],
            },
          ],
          [{ identity, module: moduleName, contract, implementation: { wait } }],
        ),
    },
  );
  const instance = new WebAssembly.Instance(module, host.imports);
  host.prime(instance);
  return {
    host,
    contract,
    module,
    moduleName,
    identity,
    target: host.imports[moduleName],
    finish(id) {
      const length = host.imports[moduleName]["wait.finish"](id);
      return new Dec(new Uint8Array(instance.exports["hd.x"].buffer, 0, length)).str();
    },
  };
}

test("pollable package operations retain results until checked delivery", () => {
  let ready = false,
    computed = 0,
    delivered = 0,
    aborted = 0;
  const h = operationHost((context) =>
    context.pending(
      () => ready,
      () => {
        computed++;
        return "computed";
      },
      () => {
        aborted++;
      },
      () => {
        delivered++;
        return "delivered";
      },
    ),
  );
  try {
    const [state, id] = h.target["wait.start"]();
    assert.equal(state, 1);
    assert.equal(h.host.core.st.ops.get(id).poll(), false);
    assert.throws(() => h.finish(id), /not pending/);
    ready = true;
    assert.equal(h.host.imports["hd:rt"].block(), 1);
    assert.equal(computed, 1);
    assert.equal(delivered, 0);
    assert.equal(h.finish(id), "delivered");
    assert.equal(delivered, 1);
    assert.equal(h.host.core.st.ops.size, 0);
    assert.throws(() => h.finish(id), /not pending/);
  } finally {
    h.host.close();
  }
  assert.equal(aborted, 0);
});

test("aborting a ready package operation cancels its undelivered resource once", () => {
  let computed = 0,
    delivered = 0,
    aborted = 0,
    signal;
  const h = operationHost((context) => {
    signal = context.signal;
    return context.pending(
      () => true,
      () => {
        computed++;
        return "held";
      },
      () => {
        assert.equal(signal.aborted, true);
        aborted++;
      },
      () => {
        delivered++;
        return "taken";
      },
    );
  });
  try {
    const [, id] = h.target["wait.start"]();
    assert.equal(h.host.imports["hd:rt"].block(), 1);
    assert.equal(computed, 1);
    h.host.imports["hd:rt"].abort(id);
    assert.equal(aborted, 1);
    assert.equal(delivered, 0);
    assert.throws(() => h.finish(id), /not pending/);
    assert.equal(h.host.core.st.ops.size, 0);
  } finally {
    h.host.close();
  }
  assert.equal(aborted, 1);
});

test("package close cancels pollable work and preserves the original core factory", () => {
  let aborted = 0,
    instanceSignal;
  const h = operationHost((context) => {
    instanceSignal = context.instanceSignal;
    return context.pending(
      () => false,
      () => {
        throw new Error("not ready");
      },
      () => {
        assert.equal(context.signal.aborted, true);
        aborted++;
      },
    );
  });
  h.target["wait.start"]();
  h.host.close();
  h.host.close();
  assert.equal(aborted, 1);
  assert.equal(instanceSignal.aborted, true);
  assert.equal(h.host.core.st.ops.size, 0);
  const shared = operationHost(() =>
    pendingOperation(
      () => true,
      () => "same core",
      () => {},
    ),
  );
  try {
    const [, id] = shared.target["wait.start"]();
    assert.equal(shared.host.imports["hd:rt"].block(), 1);
    assert.equal(shared.finish(id), "same core");
  } finally {
    shared.host.close();
  }
});

test("a failed pollable delivery is a contract failure with cleanup still owned", () => {
  let aborted = 0;
  const h = operationHost((context) =>
    context.pending(
      () => true,
      () => "held",
      () => {
        aborted++;
      },
      () => {
        throw new Error("delivery failed");
      },
    ),
  );
  try {
    const [, id] = h.target["wait.start"]();
    h.host.imports["hd:rt"].block();
    assert.throws(() => h.finish(id), /provider callback threw.*delivery failed/);
    assert.equal(h.host.core.st.ops.size, 1);
  } finally {
    h.host.close();
  }
  assert.equal(aborted, 1);
});

test("synchronous package methods cannot create or retain pending work", () => {
  const context = operationHost(
    (c) =>
      c.pending(
        () => true,
        () => "value",
      ),
    false,
  );
  try {
    assert.throws(() => context.target.wait(), /non-suspending.*pending operation/);
  } finally {
    context.host.close();
  }
  let aborted = 0;
  const direct = operationHost(
    () =>
      pendingOperation(
        () => false,
        () => "value",
        () => {
          aborted++;
        },
      ),
    false,
  );
  try {
    assert.throws(() => direct.target.wait(), /non-suspending.*pending operation/);
  } finally {
    direct.host.close();
  }
  assert.equal(aborted, 1);
});

test("pollable callback failures preserve cancellation ownership", () => {
  for (const stage of ["poll", "done", "invalid readiness"]) {
    let aborted = 0;
    const h = operationHost((context) =>
      context.pending(
        () => {
          if (stage === "poll") throw new Error("probe failed");
          return stage === "invalid readiness" ? 1 : true;
        },
        () => {
          throw new Error("result failed");
        },
        () => {
          assert.equal(context.signal.aborted, true);
          aborted++;
        },
      ),
    );
    try {
      h.target["wait.start"]();
      assert.throws(() => h.host.imports["hd:rt"].block(), HostContract);
      assert.equal(h.host.core.st.ops.size, 1);
    } finally {
      h.host.close();
    }
    assert.equal(aborted, 1);
  }
});

test("pollable factories validate callbacks before registering work", () => {
  for (const args of [
    [],
    [true, () => "value"],
    [() => true, null],
    [() => true, () => "value", 1],
    [() => true, () => "value", null, 1],
  ]) {
    const h = operationHost((context) => context.pending(...args));
    try {
      assert.throws(() => h.target["wait.start"](), HostContract);
      assert.equal(h.host.core.st.ops.size, 0);
    } finally {
      h.host.close();
    }
  }
});

// Public provider methods obtain this view through their real call context;
// none of these controls reads private host state or a compiler cache path.
function httpContext(grants) {
  let policy;
  const h = operationHost(
    (context) => {
      policy = context.http;
      return "policy observed";
    },
    false,
    grants,
  );
  h.target.wait();
  return { ...h, policy };
}

test("package context Http coverage preserves the runtime's deny, true and scoped policy", () => {
  for (const grants of [
    { Http: false },
    { Http: true },
    { Http: ["allowed.test:443", "*.example.test", "[::1]:8443"] },
    {},
  ]) {
    const { host, policy } = httpContext(grants);
    try {
      for (const [name, port] of [
        ["allowed.test", 443],
        ["allowed.test", 80],
        ["denied.test", 443],
        ["sub.example.test", 8443],
        ["::1", 8443],
      ]) {
        assert.equal(policy.coversHost(name, port), host.grant.coversHost("Http", name, port));
      }
      assert.equal(
        policy.coversHost("denied.test", 443),
        grants.Http === true || grants.Http === undefined,
      );
    } finally {
      host.close();
    }
  }
});

test("package context Http coverage uses the same core grant object and fixed Http key", () => {
  const { host, policy } = httpContext({ Http: ["allowed.test:443"], Process: true });
  const calls = [];
  const original = host.grant.coversHost;
  host.grant.coversHost = (key, name, port) => {
    calls.push([key, name, port]);
    return original(key, name, port);
  };
  try {
    assert.equal(policy.coversHost("allowed.test", 443), true);
    assert.equal(policy.coversHost("denied.test", 443, "Process"), false);
    assert.deepEqual(calls, [
      ["Http", "allowed.test", 443],
      ["Http", "denied.test", 443],
    ]);
    assert.deepEqual(Object.keys(policy).sort(), ["coversHost", "isAsciiUrl", "writtenHost"]);
  } finally {
    host.close();
  }
});

test("package context Http policy is frozen and supplies the public raw URL helpers", () => {
  const { host, policy } = httpContext({ Http: false });
  try {
    assert.equal(Object.isFrozen(policy), true);
    assert.equal(
      Reflect.set(policy, "coversHost", () => true),
      false,
    );
    assert.equal(Reflect.set(policy, "grant", {}), false);
    assert.equal(policy.coversHost("denied.test", 443), false);
    assert.equal(policy.isAsciiUrl, isAsciiUrl);
    assert.equal(policy.writtenHost, writtenHost);
    assert.equal(policy.isAsciiUrl("https://%65vil.test/"), false);
    assert.equal(policy.isAsciiUrl("https://bücher.test/"), false);
    assert.equal(policy.writtenHost("HTTP://127.1:80/path"), "127.1");
  } finally {
    host.close();
  }
});

test("package contexts cannot silently create a missing Http policy service", () => {
  const original = operationHost(() => "unused", false);
  const r = createPackageRuntime(
    { put: () => 0 },
    { module: original.module, grants: { "pkg.Source": true } },
  );
  try {
    const imports = r.bind(
      original.contract,
      [
        {
          module: original.moduleName,
          handle: false,
          methods: [{ name: "wait", member: "wait", wait: "never", params: [], result: r.str }],
        },
      ],
      [
        {
          identity: original.identity,
          module: original.moduleName,
          contract: original.contract,
          implementation: {
            wait(context) {
              return context.http.coversHost("any.test", 443) ? "allowed" : "denied";
            },
          },
        },
      ],
    );
    assert.throws(
      () => imports[original.moduleName].wait(),
      /the host does not provide HTTP policy services/,
    );
  } finally {
    r.close();
    original.host.close();
  }
});

test("package contexts cannot silently create a missing Console policy service", () => {
  const original = operationHost(() => "unused", false);
  const r = createPackageRuntime(
    { put: () => 0 },
    { module: original.module, grants: { "pkg.Source": true } },
  );
  try {
    const imports = r.bind(
      original.contract,
      [
        {
          module: original.moduleName,
          handle: false,
          methods: [{ name: "wait", member: "wait", wait: "never", params: [], result: r.str }],
        },
      ],
      [
        {
          identity: original.identity,
          module: original.moduleName,
          contract: original.contract,
          implementation: {
            wait(context) {
              return context.console.allows() ? "allowed" : "denied";
            },
          },
        },
      ],
    );
    assert.throws(
      () => imports[original.moduleName].wait(),
      /the host does not provide Console policy services/,
    );
  } finally {
    r.close();
    original.host.close();
  }
});

test("one runtime shares its immutable Http view between provider contexts", () => {
  const views = [];
  const h = operationHost(
    (context) => {
      views.push(context.http);
      assert.equal(Reflect.set(context, "http", {}), false);
      assert.equal(
        Reflect.defineProperty(context, "http", { value: { coversHost: () => true } }),
        false,
      );
      assert.equal(Reflect.deleteProperty(context, "http"), false);
      assert.equal(context.http, views.at(-1));
      return "same policy";
    },
    false,
    { Http: false },
  );
  try {
    h.target.wait();
    h.target.wait();
    assert.equal(views.length, 2);
    assert.equal(views[0], views[1]);
  } finally {
    h.host.close();
  }
});

test("a caller cannot override the context policy with an unrelated env service", () => {
  let policy;
  const forged = {
    coversHost: () => true,
    isAsciiUrl: () => true,
    writtenHost: () => "allowed.test",
  };
  const h = operationHost(
    (context) => {
      policy = context.http;
      return "fixed policy";
    },
    false,
    { Http: false },
    { http: forged },
  );
  try {
    h.target.wait();
    assert.notEqual(policy, forged);
    assert.equal(policy.coversHost("denied.test", 443), false);
  } finally {
    h.host.close();
  }
});
