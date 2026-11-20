import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import type { TestContext } from "node:test";
import { fixture } from "./node-test-support.ts";
import { Enc } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// The normal body increments memory and succeeds only when it started at zero.
// The other body loops for test 0, allowing the supervisor to stop that case.
function moduleBytes(spin = false): Uint8Array {
  const types = new Enc()
    .leb(2)
    .leb(0x60)
    .leb(1)
    .leb(0x7f)
    .leb(0)
    .leb(0x60)
    .leb(2)
    .leb(0x7f)
    .leb(0x7f)
    .leb(1)
    .leb(0x7f);
  const functions = new Enc().leb(2).leb(0).leb(1);
  const memory = new Enc().leb(1).leb(0).leb(1);
  const exports = new Enc()
    .leb(3)
    .str("hd.test.init")
    .leb(0)
    .leb(0)
    .str("hd.test")
    .leb(0)
    .leb(1)
    .str("hd.x")
    .leb(2)
    .leb(0);
  const body = spin
    ? [0, 0x20, 0, 0x45, 0x04, 0x40, 0x03, 0x40, 0x0c, 0, 0x0b, 0x0b, 0x41, 0, 0x0b]
    : [
        0, 0x41, 0, 0x41, 0, 0x28, 2, 0, 0x41, 1, 0x6a, 0x36, 2, 0, 0x41, 0, 0x28, 2, 0, 0x41, 1,
        0x47, 0x0b,
      ];
  const code = new Enc().leb(2).bytes(Uint8Array.of(0, 0x0b)).bytes(Uint8Array.from(body));
  const sections = new Enc();
  for (const [kind, data] of [
    [1, types],
    [3, functions],
    [5, memory],
    [7, exports],
    [10, code],
  ] as const) {
    sections.leb(kind).bytes(data.done());
  }
  return Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0, ...sections.done()]);
}

const counter = `
import {syncBuiltinESMExports} from "node:module";
const threads=process.getBuiltinModule("node:worker_threads");
if(threads.isMainThread){
 let compiles=0,active=0,peak=0;
 const compile=WebAssembly.compile;
 WebAssembly.compile=(...args)=>{compiles++;return Reflect.apply(compile,WebAssembly,args)};
 const OriginalWorker=threads.Worker;
 threads.Worker=class extends OriginalWorker{
  constructor(...args){super(...args);if(args[1]?.workerData?.hdTestRun){peak=Math.max(peak,++active);this.once("exit",()=>active--)}}
 };
 syncBuiltinESMExports();
 process.on("exit",()=>console.error("BATCH_WORK "+JSON.stringify({compiles,peak,active})));
}
`;

function runBatch(context: TestContext, configs: Dynamic[], options: Dynamic = {}): Dynamic {
  const root = fixture(context, {
    "counter.ts": counter,
    "normal.wasm": moduleBytes(),
    "spin.wasm": moduleBytes(true),
  });
  const programs = configs.map((config, index) => {
    const path = join(root, `config-${index}.json`);
    writeFileSync(path, JSON.stringify(config));
    return { wasm: join(root, config.spin ? "spin.wasm" : "normal.wasm"), config: path };
  });
  const manifest = join(root, "batch.json");
  writeFileSync(manifest, JSON.stringify({ node: "1.0.0", jobs: 2, programs, ...options }));
  const result = spawnSync(
    process.execPath,
    [
      "--import",
      join(root, "counter.ts"),
      fileURLToPath(new URL("./test.ts", import.meta.url)),
      "--batch",
      manifest,
    ],
    { encoding: "utf8", timeout: 20000 },
  );
  if (options.node) {
    return result;
  }
  assert.equal(result.status, 0, result.stderr);
  const records = result.stdout
    .trim()
    .split("\n")
    .map((line) => {
      const at = line.indexOf("\t");
      assert.ok(at > 0, line);
      return { program: Number(line.slice(0, at)), ...JSON.parse(line.slice(at + 1)) };
    });
  const counts = JSON.parse(result.stderr.split("BATCH_WORK ")[1].split("\n")[0]);
  return { records, counts };
}

function config(cases = [0, 1]): Dynamic {
  return {
    program: "batch",
    args: [],
    cases: cases.map((entry) => ({
      test: entry,
      init: 0,
      kind: "it",
      tempDir: null,
      seed: 1,
      grants: {},
    })),
  };
}

for (const count of [10, 100]) {
  test(
    `a ${count}-program batch compiles once per program within one worker budget`,
    { timeout: 25000 },
    (context) => {
      const { records, counts } = runBatch(
        context,
        Array.from({ length: count }, () => config()),
      );
      assert.deepEqual(counts, { compiles: count, peak: 2, active: 0 });
      assert.equal(records.length, count * 2);
      const keys = new Set(records.map((record: Dynamic) => `${record.program}:${record.test}`));
      assert.equal(keys.size, count * 2);
      assert.ok(records.every((record: Dynamic) => record.status === 0 && !record.trapped));
      // Two cases in a one-worker chunk also need distinct instance memory.
      if (count === 10) {
        const serial = runBatch(context, [config()], { jobs: 1 });
        assert.ok(serial.records.every((record: Dynamic) => record.status === 0));
        assert.equal(serial.counts.peak, 1);
      }
    },
  );
}

test(
  "a timed-out batch case does not hide its successor or another program",
  { timeout: 25000 },
  (context) => {
    const { records } = runBatch(context, [
      { ...config(), spin: true, timeLimitMs: 10 },
      config([1]),
    ]);
    assert.equal(records.length, 3);
    const cut = records.find((record: Dynamic) => record.program === 0 && record.test === 0);
    assert.equal(cut.status, 3);
    assert.equal(cut.category, "time-limit");
    assert.ok(
      records
        .filter((record: Dynamic) => record !== cut)
        .every((record: Dynamic) => record.status === 0),
    );
  },
);

test("a batch refuses an old Node before compiling any program", (context) => {
  const result = runBatch(context, [config()], { node: "999.0.0" });
  assert.equal(result.status, 78);
  assert.match(result.stderr, /hd test runs on Node 999\.0\.0 or newer/);
  const counts = JSON.parse(result.stderr.split("BATCH_WORK ")[1].split("\n")[0]);
  assert.equal(counts.compiles, 0);
});
