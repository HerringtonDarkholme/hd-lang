import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { measure, measureWarmModule } from "./run.mjs";

// (module (global (export "counter") (mut i32) (i32.const 0))
//   (func (export "hd.init") (global.set 0 (i32.add (global.get 0) (i32.const 1))))
//   (func (export "hd.poll") (param i32) (result i32)
//     (i32.sub (global.get 0) (i32.const 1))))
// A second init on a reused instance returns a nonzero status.
function counterModule() {
  const section = (id, bytes) => [id, bytes.length, ...bytes];
  const name = (text) => [text.length, ...Buffer.from(text)];
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
      ...section(1, [2, 0x60, 0, 0, 0x60, 1, 0x7f, 1, 0x7f]),
      ...section(3, [2, 0, 1]),
      ...section(6, [1, 0x7f, 1, 0x41, 0, 0x0b]),
      ...section(7, [
        3,
        ...name("hd.init"),
        0,
        0,
        ...name("hd.poll"),
        0,
        1,
        ...name("counter"),
        3,
        0,
      ]),
      ...section(
        10,
        [2, 9, 0, 0x23, 0, 0x41, 1, 0x6a, 0x24, 0, 0x0b, 7, 0, 0x23, 0, 0x41, 1, 0x6b, 0x0b],
      ),
    ]),
  );
}

test("warm activations reuse the compiled module and isolate mutable instance state", async (t) => {
  const module = counterModule();
  const instantiate = WebAssembly.instantiate;
  const instances = [];
  t.mock.method(WebAssembly, "instantiate", async (seen, imports) => {
    assert.equal(seen, module);
    const instance = await instantiate(seen, imports);
    assert.equal(instance.exports.counter.value, 0);
    instances.push(instance);
    return instance;
  });
  const result = await measureWarmModule(module, 3, "", 2);
  assert.equal(result.error, undefined);
  assert.equal(result.runs, 3);
  assert.equal(result.warmups, 2);
  assert.equal(instances.length, 5);
  assert.equal(new Set(instances).size, 5);
  for (const instance of instances) {
    assert.equal(instance.exports.counter.value, 1);
  }
});

test("warm-up stdout is validated", async () => {
  const result = await measureWarmModule(counterModule(), 1, "wrong", 1);
  assert.match(result.error, /^output /);
});

test("a failed measured activation is refused after successful warm-up", async (t) => {
  const instantiate = WebAssembly.instantiate;
  let calls = 0;
  t.mock.method(WebAssembly, "instantiate", async (...args) => {
    const instance = await instantiate(...args);
    if (++calls === 2) {
      instance.exports.counter.value = 7;
    }
    return instance;
  });
  const result = await measureWarmModule(counterModule(), 2, "", 1);
  assert.match(result.error, /^exit 7:/);
});

for (const failure of ["output", "status"]) {
  test(`fresh-process samples validate later ${failure}, beyond the preflight`, (t) => {
    const dir = mkdtempSync(join(tmpdir(), "hd-study-test-"));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    const counter = join(dir, "counter");
    const script = `
      const fs = require('node:fs');
      const file = process.argv[1];
      const n = fs.existsSync(file) ? Number(fs.readFileSync(file)) : 0;
      fs.writeFileSync(file, String(n + 1));
      if (n > 0 && ${JSON.stringify(failure)} === 'status') process.exit(7);
      process.stdout.write(n > 0 && ${JSON.stringify(failure)} === 'output' ? 'wrong' : 'ok');
    `;
    const result = measure([process.execPath, "-e", script, counter], 2, "ok");
    assert.match(result.error, failure === "output" ? /^output / : /^exit 7:/);
  });
}
