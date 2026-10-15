// Smoke test of the playground's compiler wasm (playground-wasm.md §7, W3).
// Loads website/playground/assets/hd_web.wasm (build-web-compiler.js writes
// it), compiles a conformance fixture through the four exports (the
// playground's own wire module, compiler-wire.ts), runs the
// program with host/run.mjs as `hd run` on Node does, and checks its
// stdout. Then a two-input REPL session through the REPL goals, each
// module run in the JS host's session (host/core.mjs, W4) before its
// outcome goes back to the compiler. Then the size
// gate: an LTO build that shrinks (a panicking call such as
// `Instant::now()` deletes everything after it) fails the floor.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  GOAL,
  OUTCOME,
  STATUS,
  compileRequest,
  encodeReplInput,
  encodeReplOutcome,
  encodeReplStart,
  encodeRequest,
} from "../website/playground/src/compiler-wire.ts";
import { COMPILER_WASM, formatBytes, sizes } from "./build-web-compiler.js";
import { createNodeSession } from "../host/node.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = join(root, "spec", "conformance", "runtime", "valid", "println-console-stdout.hd");

// The plan's 3.2 MB raw came from the `stages` example, which reads std
// from disk; the real compiler embeds std's 0.51 MB of source (§2). W8's
// wasmprinter added 0.49 MB raw and 0.13 MB gzip (3.90 -> 4.39 MB, 1.25 ->
// 1.39 MB), so the gates are 0.5 MB and 0.15 MB higher than W3's. The REPL
// session support in the driver and linker (#17, C2-C8) added 0.12 MB raw
// and 0.04 MB gzip (4.39 -> 4.51 MB, 1.39 -> 1.42 MB); the playground's REPL
// panel will use it, so the raw gate is 4.7 MB. The REPL goals (W3) added
// 11 KB raw and 5 KB gzip (4.550 -> 4.560 MB, 1.436 -> 1.440 MB), within
// both gates. The REPL's session-as-one-module rules for derivations, facts
// and drift (#159, f101ce35 onward) took gzip to 1.454 MB, so the gzip gate
// is 1.5 MB. The std skim cache and per-process std sources (#49, b068b071)
// added 15 KB raw (4.696 -> 4.711 MB); the browser compiler keeps one driver
// across compiles, so warm compiles use them too. The raw gate is 4.75 MB.
const RAW_LIMIT = 4.75e6;
const GZIP_LIMIT = 1.5e6;
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

const printed = compile(exports, encodeRequest({ "src/main.hd": text }, "src/main.hd", GOAL.wat));
assert.equal(printed.status, STATUS.ok);
assert.ok(printed.wat.startsWith("(module"), printed.wat.slice(0, 40));
assert.deepEqual(printed.wasm, cold.wasm);

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

// A REPL session: the prelude, then two inputs, each run in the JS host's
// session and reported as joined.
let replOut = "";
const session = createNodeSession({
  out: (text) => {
    replOut += text;
  },
  err: (text) => {
    replOut += text;
  },
});
const wasmMagic = [0, 0x61, 0x73, 0x6d];
const prelude = compile(exports, encodeReplStart({ "src/lib.hd": "fn base() -> i32: +40\n" }));
assert.equal(prelude.status, STATUS.ok, JSON.stringify(prelude.json));
assert.equal(prelude.json.input, 0);
assert.ok(prelude.json.sessionKeys.includes("hd.x"), "the prelude exports the memory");
assert.deepEqual([...prelude.wasm.subarray(0, 4)], wasmMagic);
assert.equal((await session.input(0, "", prelude.wasm)).joined, true);
let keys = prelude.json.sessionKeys.length;
const joined = compile(exports, encodeReplOutcome(0, OUTCOME.joined));
assert.deepEqual(joined.json, { diagnostics: [], input: 0, joined: true });
for (const [n, text] of [
  [1, "n := base() + 2\n"],
  [2, "n * 2\n"],
]) {
  const input = compile(exports, encodeReplInput(text));
  assert.equal(input.status, STATUS.ok, JSON.stringify(input.json));
  assert.equal(input.json.input, n);
  assert.deepEqual([...input.wasm.subarray(0, 4)], wasmMagic);
  keys += input.json.sessionKeys.length;
  assert.equal((await session.input(n, text, input.wasm)).joined, true, replOut);
  assert.equal(compile(exports, encodeReplOutcome(n, OUTCOME.joined)).status, STATUS.ok);
}
assert.equal(replOut, "84 : i32\n");
session.close();
const redeclared = compile(exports, encodeReplInput("n := 1\n"));
assert.equal(redeclared.status, STATUS.errors);
assert.equal(redeclared.json.diagnostics[0].path, "repl#3");
assert.equal(redeclared.wasm.length, 0);
const early = compile(exports, encodeReplOutcome(9, OUTCOME.joined));
assert.equal(early.status, STATUS.malformed);
console.log(
  `repl: prelude ${prelude.wasm.length} B, ${keys} session keys, cold ${prelude.ms.toFixed(0)} ms`,
);

assert.ok(raw <= RAW_LIMIT, `raw ${formatBytes(raw)} is over ${formatBytes(RAW_LIMIT)}`);
assert.ok(gzip <= GZIP_LIMIT, `gzip ${formatBytes(gzip)} is over ${formatBytes(GZIP_LIMIT)}`);
assert.ok(
  raw >= RAW_FLOOR,
  `raw ${formatBytes(raw)} is under ${formatBytes(RAW_FLOOR)}: LTO shrink`,
);
console.log("web compiler smoke test passed");
