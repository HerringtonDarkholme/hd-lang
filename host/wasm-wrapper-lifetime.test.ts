// Node 24's import-wrapper code GC can free a dying wrapper twice as another
// isolate tiers up a wrapper. Keep worker teardown and wrapper tiering concurrent.
// https://github.com/v8/v8/commit/9b8ca54d5a6b6c615eb7ad6d9e2bdca7f8cb1a81
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import test from "node:test";
import { promisify } from "node:util";

const run = promisify(execFile);

test("Wasm import wrappers survive code GC while workers exit", { timeout: 45_000 }, async () => {
  const worker = `
    import { parentPort, workerData } from 'node:worker_threads';
    let sum = 0;
    for (let generation = 0; generation < 16; generation++) {
      let instance = new WebAssembly.Instance(workerData.module, {env: {f: x => x + 1}});
      for (let value = 0; value < 64; value++) sum += instance.exports.run(value);
      instance = null;
      globalThis.gc();
    }
    parentPort.postMessage(sum);
    parentPort.close();
  `;
  const program = `
    import assert from 'node:assert/strict';
    import { Worker } from 'node:worker_threads';
    const bytes = new Uint8Array([
      0,97,115,109,1,0,0,0,
      1,6,1,96,1,127,1,127,
      2,9,1,3,101,110,118,1,102,0,0,
      3,2,1,0,
      7,7,1,3,114,117,110,0,1,
      10,8,1,6,0,32,0,16,0,11,
    ]);
    const module = new WebAssembly.Module(bytes);
    const source = new URL('data:text/javascript,' + encodeURIComponent(${JSON.stringify(worker)}));
    async function lane() {
      let sum = 0;
      for (let generation = 0; generation < 20; generation++) {
        sum += await new Promise((resolve, reject) => {
          const worker = new Worker(source, {workerData: {module}, execArgv: []});
          let result;
          worker.once('message', value => { result = value; });
          worker.once('error', reject);
          worker.once('exit', code => {
            if (code !== 0) reject(new Error('worker exit ' + code));
            else if (result === undefined) reject(new Error('worker exited without its result'));
            else resolve(result);
          });
        });
      }
      return sum;
    }
    const sums = await Promise.all([lane(), lane()]);
    assert.equal(sums[0] + sums[1], 2 * 20 * 16 * (64 * 65 / 2));
    console.log(JSON.stringify({workers: 40, calls: 40 * 16 * 64}));
  `;
  // cost: one process, two concurrent workers, 40 worker lifetimes and 40,960
  // imported calls and 640 forced GCs; independent of the repository or previously compiled modules.
  const { stdout } = await run(
    process.execPath,
    [
      "--expose-gc",
      "--stress-wasm-code-gc",
      "--wasm-wrapper-tiering-budget=0",
      "--wasm-num-compilation-tasks=2",
      "--input-type=module",
      "-e",
      program,
    ],
    { timeout: 30_000 },
  );
  assert.deepEqual(JSON.parse(stdout), { workers: 40, calls: 40_960 });
});
