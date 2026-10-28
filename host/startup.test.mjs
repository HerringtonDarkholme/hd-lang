import assert from "node:assert/strict";
import { test } from "node:test";
import { HostContract } from "./run-support.mjs";
import { createStartupHost } from "./startup.mjs";
import { Dec } from "./wire.mjs";

const module = new WebAssembly.Module(
  Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0, 1, 13, 3, 96, 0, 0, 96, 1, 127, 1, 127, 96, 1, 127, 0, 2, 30, 2, 5,
    104, 100, 58, 114, 116, 5, 112, 97, 110, 105, 99, 0, 0, 5, 104, 100, 58, 114, 116, 6, 115, 116,
    100, 101, 114, 114, 0, 2, 3, 3, 2, 0, 1, 5, 3, 1, 0, 1, 7, 28, 3, 4, 104, 100, 46, 120, 2, 0, 7,
    104, 100, 46, 105, 110, 105, 116, 0, 2, 7, 104, 100, 46, 112, 111, 108, 108, 0, 3, 10, 9, 2, 2,
    0, 11, 4, 0, 65, 0, 11,
  ]),
);

test("panic-only modules skip the provider host", async () => {
  const text = [];
  const host = await createStartupHost(module, {
    out: (value) => text.push(value),
    err: (value) => text.push(value),
  });
  assert.deepEqual(Object.keys(host.imports), ["hd:rt"]);
  const instance = new WebAssembly.Instance(module, host.imports);
  assert.deepEqual(await host.run(instance, "hd.init", "hd.poll"), {
    status: 0,
    trapped: false,
    ended: false,
    frames: [],
    panic: null,
    cancelled: false,
  });
  assert.deepEqual(text, []);
  host.close();
});

test("malformed wire arguments keep the shared host-contract error", () => {
  assert.throws(() => new Dec(new Uint8Array()).leb(), HostContract);
});
