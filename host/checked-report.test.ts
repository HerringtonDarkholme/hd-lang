import assert from "node:assert/strict";
import test from "node:test";
import { cycleReport } from "./checked-report.ts";

function moduleWith(data) {
  const name = [...new TextEncoder().encode("hd.cycles")];
  const custom = [name.length, ...name, ...data];
  return new WebAssembly.Module(
    new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 0, custom.length, ...custom]),
  );
}
function instance() {
  return {
    exports: {
      "hd.heap": new WebAssembly.Memory({ initial: 1 }),
      "hd.cycles.pointer": () => 8,
      "hd.cycles.length": () => 3,
    },
  };
}
test("descriptor aliases retain distinct kinds and full-width totals", () => {
  const module = moduleWith([1, 1, 0, 0, 0, 1, 2, 0, 0, 0, 2]);
  const value = instance();
  const view = new DataView(value.exports["hd.heap"].buffer);
  view.setBigUint64(16, 1n << 60n, true);
  view.setBigUint64(24, 3n, true);
  assert.equal(
    cycleReport(module, value),
    "cycles freed: 1152921504606846976 closure envs, 3 local cells, 0 frames, 0 waker/all nodes, 0 list/map buffers, 0 list/map headers, 0 objects\n",
  );
});
test("absent or malformed metadata is ignored", () => {
  assert.equal(cycleReport(null, instance()), null);
  for (const bytes of [
    [2],
    [1, 1],
    [1, 0, 0, 0, 0, 1],
    [1, 1, 0, 0, 0, 7],
    [1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 2],
  ]) {
    assert.equal(cycleReport(moduleWith(bytes), instance()), null);
  }
});
test("an invalid counter range is ignored", () => {
  const value = instance();
  value.exports["hd.cycles.pointer"] = () => 65535;
  assert.equal(cycleReport(moduleWith([1]), value), null);
});
