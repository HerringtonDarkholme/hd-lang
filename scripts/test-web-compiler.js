// Smoke test of the playground's compiler wasm (playground-wasm.md §7, W3).
// Loads website/playground/assets/hd_web.wasm (build-web-compiler.js writes
// it), compiles a conformance fixture through the four exports (the
// playground's own wire module, compiler-wire.ts), runs the
// program with host/run.mjs as `hd run` on Node does, and checks its
// stdout. Then the size gate: an LTO build that shrinks (a panicking call
// such as `Instant::now()` deletes everything after it) fails the floor.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  GOAL,
  STATUS,
  compileRequest,
  encodeRequest,
} from "../website/playground/src/compiler-wire.ts";
import { COMPILER_WASM, formatBytes, sizes } from "./build-web-compiler.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = join(root, "spec", "conformance", "runtime", "valid", "println-console-stdout.hd");

// The plan's 3.2 MB raw came from the `stages` example, which reads std
// from disk; the real compiler embeds std's 0.51 MB of source (§2).
const RAW_LIMIT = 4e6;
const GZIP_LIMIT = 1.3e6;
const RAW_FLOOR = 2e6;

/** Compiles `request` and times the call. */
function compile(exports, request) {
  const start = performance.now();
  const compiled = compileRequest(exports, request);
  return { ...compiled, ms: performance.now() - start };
}

const { raw, gzip } = sizes(COMPILER_WASM);
console.log(`hd_web.wasm: raw ${formatBytes(raw)}, gzip ${formatBytes(gzip)}`);

const { instance } = await WebAssembly.instantiate(readFileSync(COMPILER_WASM), {});
const exports = instance.exports;
const text = readFileSync(FIXTURE, "utf8");
const request = encodeRequest({ "src/main.hd": text }, "src/main.hd", GOAL.run);

const cold = compile(exports, request);
assert.deepEqual(cold.json, { diagnostics: [] });
assert.equal(cold.status, STATUS.ok);
assert.deepEqual([...cold.wasm.subarray(0, 4)], [0, 0x61, 0x73, 0x6d]);
const warm = compile(exports, request);
assert.equal(warm.status, STATUS.ok);
console.log(
  `compile: cold ${cold.ms.toFixed(0)} ms, warm ${warm.ms.toFixed(0)} ms; program ${cold.wasm.length} B`,
);

const dir = join(root, "target", "web-compiler");
mkdirSync(dir, { recursive: true });
const program = join(dir, "main.wasm");
writeFileSync(program, cold.wasm);
const stdout = execFileSync(process.execPath, [join(root, "host", "run.mjs"), program], {
  encoding: "utf8",
});
const expected = [...text.matchAll(/^# expect-stdout: (.*)$/gm)].map((m) => `${m[1]}\n`).join("");
assert.equal(stdout, expected);
console.log(`run: ${JSON.stringify(stdout)}`);

const broken = compile(
  exports,
  encodeRequest({ "src/main.hd": "x: i32 = true\n" }, "src/main.hd", GOAL.check),
);
assert.equal(broken.status, STATUS.errors);
assert.equal(broken.json.diagnostics[0].path, "src/main.hd");
assert.equal(broken.wasm.length, 0);
const malformed = compile(exports, Uint8Array.of(1));
assert.equal(malformed.status, STATUS.malformed);
assert.deepEqual(malformed.json, { error: "request ends early" });

assert.ok(raw <= RAW_LIMIT, `raw ${formatBytes(raw)} is over ${formatBytes(RAW_LIMIT)}`);
assert.ok(gzip <= GZIP_LIMIT, `gzip ${formatBytes(gzip)} is over ${formatBytes(GZIP_LIMIT)}`);
assert.ok(
  raw >= RAW_FLOOR,
  `raw ${formatBytes(raw)} is under ${formatBytes(RAW_FLOOR)}: LTO shrink`,
);
console.log("web compiler smoke test passed");
