import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { runCases } from "./cases.ts";
import type { CaseConfig, CaseRecord, TestFiles } from "./cases.ts";
import type { Host, Sink } from "./core.ts";
import { createHost, nodeFiles } from "./node.ts";
import { fixture } from "./node-test-support.ts";
import { createWebHost, memoryFiles } from "./web.ts";
import { Dec, Enc } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// The normal test entry calls a fixture callback through a real Wasm frame.
const types = new Enc().leb(3);
types.leb(0x60).leb(0).leb(1).leb(0x7f);
types.leb(0x60).leb(1).leb(0x7f).leb(0);
types.leb(0x60).leb(2).leb(0x7f).leb(0x7f).leb(1).leb(0x7f);
const imports = new Enc().leb(1).str("fixture").str("body").leb(0).leb(0);
const functions = new Enc().leb(2).leb(1).leb(2);
const memory = new Enc().leb(1).leb(0).leb(1);
const exports = new Enc()
  .leb(3)
  .str("hd.test.init")
  .leb(0)
  .leb(1)
  .str("hd.test")
  .leb(0)
  .leb(2)
  .str("hd.x")
  .leb(2)
  .leb(0);
const code = new Enc()
  .leb(2)
  .bytes(Uint8Array.of(0, 0x0b))
  .bytes(Uint8Array.of(0, 0x10, 0, 0x0b));
const sections = new Enc();
for (const [kind, section] of [
  [1, types],
  [2, imports],
  [3, functions],
  [5, memory],
  [7, exports],
  [10, code],
] as const) {
  sections.leb(kind).bytes(section.done());
}
const module = new WebAssembly.Module(
  Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0, ...sections.done()]),
);

function config(tempDir: string | null, kind = "it"): CaseConfig {
  return { test: 0, init: 0, kind, tempDir, seed: 1, grants: {} };
}

async function run(
  cases: CaseConfig[],
  body: (host: Host, env: Dynamic) => number,
  files: TestFiles = nodeFiles,
  make: (sink: Sink, env: Dynamic) => Host = createHost,
) {
  const removed: string[] = [],
    records: CaseRecord[] = [];
  await runCases({
    module,
    config: { program: "cleanup", args: [], cases },
    files: {
      ...files,
      remove(dir) {
        removed.push(dir);
        files.remove(dir);
      },
    },
    notify() {},
    emit: (record) => records.push(record),
    createHost(sink, env) {
      let host: Host;
      env.files = files;
      env.providers = () => ({ fixture: { body: () => body(host, env) } });
      host = make(sink, env);
      return host;
    },
  });
  return { removed, records };
}

for (const count of [1000, 10000]) {
  test(`unused test directories issue zero removes at ${count} bodies`, async (context) => {
    const root = fixture(context, {});
    const cases = Array.from({ length: count }, (_, i) => config(join(root, String(i))));
    const { removed, records } = await run(cases, () => 0);
    assert.equal(removed.length, 0);
    assert.equal(records.length, count);
    assert.ok(records.every((record) => record.status === 0));
  });
}

for (const outcome of ["success", "failure", "trap"]) {
  test(`requested Node test directory is removed after ${outcome}`, async (context) => {
    const dir = join(fixture(context, {}), "case");
    const { removed, records } = await run([config(dir)], (host) => {
      assert.equal(host.tempDir?.(), dir);
      assert.equal(host.tempDir?.(), dir);
      writeFileSync(join(dir, "kept.txt"), "body");
      if (outcome === "trap") {
        throw new WebAssembly.RuntimeError("fixture trap");
      }
      return outcome === "failure" ? 1 : 0;
    });
    assert.deepEqual(removed, [dir]);
    assert.equal(existsSync(dir), false);
    assert.equal(records.length, 1);
    assert.equal(records[0].status === 0, outcome === "success");
  });
}

for (const kind of ["it_each", "it_prop"]) {
  test(`directory creation state resets across ${kind} bodies`, async (context) => {
    const dir = join(fixture(context, {}), "case");
    const used: string[] = [];
    let bodies = 0;
    const { removed, records } = await run([config(dir, kind)], (host, env) => {
      const index = kind === "it_each" ? env.runner.row(4) : env.runner.start(4, 0, 0).size;
      bodies++;
      if (index % 2 === 0) {
        const path = host.tempDir?.();
        assert.ok(path);
        used.push(path);
        writeFileSync(join(path, "kept.txt"), "body");
      }
      return 0;
    });
    assert.equal(bodies, 4);
    assert.deepEqual(used, [dir, join(dir, "2")]);
    assert.deepEqual(removed, used);
    assert.ok(used.every((path) => !existsSync(join(path, "kept.txt"))));
    assert.ok(records.every((record) => record.status === 0));
  });
}

test("standalone host-owned temporary directories still get cleaned", async () => {
  let dir = "";
  const { removed } = await run([config(null)], (host) => {
    dir = host.tempDir?.() ?? "";
    assert.ok(dir);
    writeFileSync(join(dir, "kept.txt"), "body");
    return 0;
  });
  assert.deepEqual(removed, [dir]);
  assert.equal(existsSync(dir), false);
});

test("portable in-memory host marks requests and clears each row's files", async () => {
  const files = memoryFiles(),
    used: string[] = [];
  const { removed, records } = await run(
    [config("/case", "it_each")],
    (host, env) => {
      const index = env.runner.row(4);
      if (index % 2 === 0) {
        const length = host.imports["hd:TestRunner"].temp_dir();
        const memory = host.core.st.memory as WebAssembly.Memory;
        assert.ok(length > 0);
        const path = new Dec(new Uint8Array(memory.buffer, 0, length)).str();
        assert.equal(path, index === 0 ? "/case" : "/case/2");
        assert.equal(host.core.st.tempMade, true);
        used.push(path);
        files.write(`${path}/kept.txt`, "body");
      }
      return 0;
    },
    files,
    createWebHost,
  );
  assert.deepEqual(removed, used);
  assert.equal(used.length, 2);
  assert.ok(used.every((path) => files.read(`${path}/kept.txt`) === null));
  assert.ok(records.every((record) => record.status === 0));
});

for (const count of [1000, 10000]) {
  test(`an unused portable directory skips cleanup over ${count} saved files`, async () => {
    const files = memoryFiles();
    for (let i = 0; i < count; i++) {
      files.write(`/snapshots/${i}`, "saved");
    }
    const { removed, records } = await run([config("/case")], () => 0, files, createWebHost);
    assert.deepEqual(removed, []);
    assert.equal(records[0].status, 0);
    assert.equal(files.read(`/snapshots/${count - 1}`), "saved");
  });
}
