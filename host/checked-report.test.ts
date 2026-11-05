import assert from "node:assert/strict";
import test from "node:test";
import { countPairRows, cycleReport } from "./checked-report.ts";

function moduleWith(data: number[], section = "hd.cycles"): WebAssembly.Module {
  const name = [...new TextEncoder().encode(section)];
  const custom = [name.length, ...name, ...data];
  return new WebAssembly.Module(
    new Uint8Array([
      0,
      97,
      115,
      109,
      1,
      0,
      0,
      0,
      0,
      ...(custom.length < 128
        ? [custom.length]
        : [(custom.length & 127) | 128, custom.length >>> 7]),
      ...custom,
    ]),
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

function countSite(id: number, fn: string, file: string, line: number): number[] {
  const u32 = (value: number) => [
    value & 255,
    (value >>> 8) & 255,
    (value >>> 16) & 255,
    value >>> 24,
  ];
  const text = (value: string) => {
    const bytes = [...new TextEncoder().encode(value)];
    return [...u32(bytes.length), ...bytes];
  };
  return [...u32(id), ...text(fn), ...text(file), ...u32(line), ...text("source call")];
}
function countInstance() {
  return {
    exports: {
      "hd.heap": new WebAssembly.Memory({ initial: 1 }),
      "hd.count.pairs.pointer": () => 8,
      "hd.count.pairs.length": () => 4,
    },
  };
}
test("count pairs rank source lines and combine full-width site totals", () => {
  const value = countInstance();
  const view = new DataView(value.exports["hd.heap"].buffer);
  view.setBigUint64(16, 1n << 60n, true);
  view.setBigUint64(24, 3n, true);
  view.setBigUint64(32, 4n, true);
  const metadata = [
    1,
    ...countSite(1, "f", "f.hd", 9),
    ...countSite(2, "f", "f.hd", 9),
    ...countSite(3, "g", "g.hd", 2),
  ];
  assert.deepEqual(countPairRows(moduleWith(metadata, "hd.count.sites"), value), [
    { function: "f", file: "f.hd", line: 9, reason: "source call", count: "1152921504606846979" },
    { function: "g", file: "g.hd", line: 2, reason: "source call", count: "4" },
  ]);
});
test("count reports reject malformed sites and out-of-bounds counters", () => {
  const value = countInstance();
  for (const metadata of [
    [2],
    [1, 1],
    [1, ...countSite(0, "f", "f.hd", 1)],
    [1, ...countSite(1, "f", "f.hd", 1), ...countSite(1, "f", "f.hd", 2)],
  ]) {
    assert.equal(countPairRows(moduleWith(metadata, "hd.count.sites"), value), null);
  }
  value.exports["hd.count.pairs.pointer"] = () => 65535;
  assert.equal(countPairRows(moduleWith([1], "hd.count.sites"), value), null);
});

test("a missed emitting site is visible instead of silently discarded", () => {
  const value = countInstance();
  new DataView(value.exports["hd.heap"].buffer).setBigUint64(8, 7n, true);
  assert.deepEqual(countPairRows(moduleWith([1], "hd.count.sites"), value), [
    {
      function: "<unattributed>",
      file: "<runtime>",
      line: 0,
      reason: "instrumentation gap",
      count: "7",
    },
  ]);
});
