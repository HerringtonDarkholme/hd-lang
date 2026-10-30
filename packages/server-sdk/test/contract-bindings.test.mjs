import assert from "node:assert/strict";
import { test, after } from "node:test";
import { rm } from "node:fs/promises";
import { contractBindings } from "../packaging/contract-bindings.mjs";
import { checkCompanionPairing, fixedBindingsModule } from "../packaging/companion.mjs";
import { readPackageContract, createPackageRuntime } from "../../../host/package-providers.mjs";
import { Enc, Dec } from "../../../host/wire.mjs";
import { fixture } from "./helpers/deployment-fixture.mjs";
const compiled = fixture();
after(() => rm(compiled.directory, { recursive: true, force: true }));
const module = new WebAssembly.Module(compiled.wasm);
const actual = new Uint8Array(WebAssembly.Module.customSections(module, "hd.capabilities")[0]);
const companion = compiled.companion;
import { encode as encodeContract } from "./helpers/deployment-contract.mjs";
function wasmContract(bytes) {
  const custom = new Uint8Array([...new Enc().str("hd.capabilities").done(), ...bytes]);
  return new WebAssembly.Module(
    new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, ...new Enc().leb(0).bytes(custom).done()]),
  );
}
const handleName = "hd:pkg:returned-handle";
const handle = {
  identity: { package: "tests/sdk", version: "1.0.0", trait_path: "Handle" },
  module: handleName,
  selectors: [],
  handle: true,
  methods: [{ name: "echo", member: "echo", wait: "never", params: [], result: [6] }],
};
const root = {
  identity: { package: "tests/sdk", version: "1.0.0", trait_path: "Source" },
  module: "hd:pkg:source",
  selectors: ["pkg.Source"],
  handle: false,
  methods: [],
};
function descriptors(runtime, binding, providers) {
  let capture;
  const wrapper = {
    ...runtime,
    bind(...args) {
      capture = args;
      return "bound";
    },
  };
  assert.equal(binding.bind(wrapper, providers), "bound");
  return capture;
}
test("the real compiler companion and data-derived bindings have identical signatures and registrations", async () => {
  const generated = await import("data:text/javascript," + encodeURIComponent(companion));
  const derived = contractBindings(actual, readPackageContract),
    providers = new Map([[derived.interfaces[0].module, {}]]);
  assert.deepEqual(derived.interfaces, generated.interfaces);
  const r = createPackageRuntime({});
  const normalize = ([bytes, shapes, registrations]) => ({
    bytes,
    shapes: shapes.map((i) => ({
      ...i,
      methods: i.methods.map((m) => ({
        ...m,
        params: m.params.map((c) => c.shape),
        result: m.result.shape,
      })),
    })),
    registrations,
  });
  assert.deepEqual(
    normalize(descriptors(r, derived, providers)),
    normalize(descriptors(r, generated, providers)),
  );
});
test("all 16 codec tags, integer widths and nested value shapes retain canonical descriptors", () => {
  const types = [
    [0],
    [1],
    [2],
    [3, 8, false],
    [4],
    [5],
    [6],
    [7],
    [8, [6]],
    [9, [3, 64, true]],
    [10, [[1], [6]]],
    [
      11,
      [
        ["__proto__", [6]],
        ["line\u2028field", [3, 32, true]],
      ],
    ],
    [
      12,
      [
        ["Empty", []],
        ["Payload", [["value", [8, [6]]]]],
      ],
    ],
    [13, [9, [6]], [12, [["Bad", [["why", [6]]]]]]],
    [14],
    [15, handleName],
  ];
  for (const bits of [8, 16, 32, 64]) {
    for (const signed of [false, true]) {
      types.push([3, bits, signed]);
    }
  }
  const source = {
    ...root,
    methods: types.map((result, i) => ({
      name: "value" + i,
      member: "value" + i,
      wait: i % 2 ? "may" : "never",
      params: [],
      result,
    })),
  };
  const bytes = encodeContract([source, handle]),
    binding = contractBindings(bytes, readPackageContract);
  const [, shapes, registrations] = descriptors(
    createPackageRuntime({}),
    binding,
    new Map([[root.module, {}]]),
  );
  assert.deepEqual(
    shapes[0].methods.map((m) => m.result.shape),
    types,
  );
  assert.deepEqual(
    shapes[0].methods.map((m) => m.wait),
    source.methods.map((m) => m.wait),
  );
  assert.deepEqual(registrations[0].identity, root.identity);
  assert.deepEqual(registrations[0].contract, bytes);
});
test("input and exported contract mutation cannot change the checked bind contract or identities", () => {
  const source = actual.slice(),
    binding = contractBindings(source, readPackageContract);
  source.fill(0);
  binding.contract.fill(0);
  assert.throws(() => (binding.interfaces[0].identity.package = "foreign"), TypeError);
  assert.throws(() => binding.interfaces[0].selectors.push("pkg.Foreign"), TypeError);
  const [bytes, , registrations] = descriptors(
    createPackageRuntime({}),
    binding,
    new Map([[binding.interfaces[0].module, {}]]),
  );
  assert.deepEqual(bytes, actual);
  assert.deepEqual(registrations[0].contract, actual);
});
test("root provider map requirements match generated bindings", () => {
  const binding = contractBindings(actual, readPackageContract),
    r = createPackageRuntime({});
  assert.throws(() => binding.bind(r, {}), /must be a map/);
  assert.throws(() => binding.bind(r, new Map([["foreign", {}]])), /no required/);
  assert.throws(() => binding.bind(r, new Map()), /missing package/);
});
test("opaque returned handles invoke only their declared namespace and release without leaking", () => {
  const source = {
    ...root,
    methods: [
      { name: "open", member: "open", wait: "never", params: [], result: [15, handleName] },
    ],
  };
  const bytes = encodeContract([source, handle]),
    binding = contractBindings(bytes, readPackageContract);
  let output;
  const runtime = createPackageRuntime(
    {
      put(e) {
        output = e.done();
        return output.length;
      },
    },
    { module: wasmContract(bytes), grants: { "pkg.Source": true } },
  );
  const imports = binding.bind(
    runtime,
    new Map([
      [
        root.module,
        {
          open() {
            return {
              echo() {
                return "owned handle";
              },
            };
          },
        },
      ],
    ]),
  );
  imports[root.module].open();
  const id = new Dec(output).num();
  assert.equal(runtime.handleCount(), 1);
  imports[handleName].echo(id);
  assert.equal(new Dec(output).str(), "owned handle");
  assert.throws(() => imports[handleName].echo(id + 1), /invalid handle/);
  imports[handleName].$release(id);
  assert.equal(runtime.handleCount(), 0);
  assert.throws(() => imports[handleName].echo(id), /invalid handle/);
  runtime.close();
});
test("a malformed returned handle rolls back allocation", () => {
  const source = {
    ...root,
    methods: [
      { name: "open", member: "open", wait: "never", params: [], result: [15, handleName] },
    ],
  };
  const bytes = encodeContract([source, handle]),
    binding = contractBindings(bytes, readPackageContract);
  const runtime = createPackageRuntime(
    {
      put(e) {
        return e.done().length;
      },
    },
    { module: wasmContract(bytes), grants: { "pkg.Source": true } },
  );
  const imports = binding.bind(
    runtime,
    new Map([
      [
        root.module,
        {
          open() {
            return {};
          },
        },
      ],
    ]),
  );
  assert.throws(() => imports[root.module].open(), /missing a declared method/);
  assert.equal(runtime.handleCount(), 0);
  runtime.close();
});
test("malformed contract metadata is rejected by the canonical data reader", () => {
  for (const bytes of [
    new Uint8Array(),
    new Uint8Array([2, 0, 0, 0, 0]),
    actual.subarray(0, actual.length - 1),
    new Uint8Array([...actual, 0]),
  ]) {
    assert.throws(() => contractBindings(bytes, readPackageContract));
  }
  const source = {
    ...root,
    methods: [
      { name: "bad", member: "bad", wait: "never", params: [], result: [15, "hd:pkg:absent"] },
    ],
  };
  assert.throws(
    () => contractBindings(encodeContract([source]), readPackageContract),
    /missing handle/,
  );
});
test("companion metadata pairing is pure and rejects malformed or mismatched headers", () => {
  checkCompanionPairing(companion, actual);
  checkCompanionPairing(companion.replaceAll("\n", "\r\n"), actual);
  for (const source of [
    companion.replace("abiVersion = 1", "abiVersion = 2"),
    companion.replace("Uint8Array([1,", "Uint8Array([2,"),
    companion.replace("Uint8Array([1,", "Uint8Array([256,"),
    "export const contract=alert(1);",
    companion.replace("Uint8Array([1,", "Uint8Array([1,,"),
  ]) {
    assert.throws(() => checkCompanionPairing(source, actual));
  }
  assert.throws(() => checkCompanionPairing(new Uint8Array([255]), actual));
  assert.throws(() => fixedBindingsModule(actual, "../outside"), /unknown fixed/);
});
test("Node Buffer inputs are copied rather than retained through Buffer.slice", () => {
  const source = Buffer.from(actual);
  const binding = contractBindings(source, readPackageContract);
  source.fill(0);
  const [bytes] = descriptors(
    createPackageRuntime({}),
    binding,
    new Map([[binding.interfaces[0].module, {}]]),
  );
  assert.deepEqual(new Uint8Array(bytes), actual);
});
