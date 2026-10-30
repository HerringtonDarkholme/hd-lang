// Smoke test of the playground's compiler wasm (crates/hd_web/DESIGN.md §7, W3).
// Loads website/playground/assets/hd_web.wasm (build-web-compiler.js writes
// it), compiles a conformance fixture through the four exports (the
// playground's own wire module, compiler-wire.ts), runs the
// program with host/run.mjs as `hd run` on Node does, and checks its
// stdout. Then a two-input REPL session through the REPL goals, each
// session program run in the JS host's session (host/core.mjs) before
// its outcome goes back to the compiler. Then the size
// gate: an LTO build that shrinks (a panicking call such as
// `Instant::now()` deletes everything after it) fails the floor.

import { compilerImports, instantiateCompiler } from "../website/playground/src/compiler-host.ts";

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
import { COMPILER_WASM, WAT_WASM, formatBytes, sizes } from "./build-web-compiler.js";
import { createWatPrinter } from "../website/playground/src/wat-printer.ts";
import { createNodeSession } from "../host/node-session.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = join(root, "spec", "conformance", "runtime", "valid", "println-console-stdout.hd");

// #852 removes the printer from ordinary compilation. These caps keep
// modest margins above the measured 1,851,877 (#1026) / 147,828 gzip bytes.
// The owner's hard 2 MB ceiling still holds.
const GZIP_LIMIT = 1_900_000;
const WAT_GZIP_LIMIT = 160_000;
const RAW_FLOOR = 2e6;

/** Compiles `request` and times the call. */
function compile(exports, request) {
  const start = performance.now();
  const compiled = compileRequest(exports, request);
  return { ...compiled, ms: performance.now() - start };
}

const { raw, gzip } = sizes(COMPILER_WASM);
const watSize = sizes(WAT_WASM);
console.log(`hd_wat.wasm: raw ${formatBytes(watSize.raw)}, gzip ${formatBytes(watSize.gzip)}`);
console.log(`hd_web.wasm: raw ${formatBytes(raw)}, gzip ${formatBytes(gzip)}`);

const { instance } = await instantiateCompiler(readFileSync(COMPILER_WASM));
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

let printerLoads = 0;
const printWat = createWatPrinter(async () => {
  printerLoads += 1;
  return readFileSync(WAT_WASM);
});
assert.equal(printerLoads, 0, "Run never loads the printer");
const printed = await printWat(cold.wasm);
assert.ok(printed.startsWith("(module"), printed.slice(0, 40));
assert.equal(await printWat(cold.wasm), printed);
assert.equal(printerLoads, 1, "repeated WAT requests share the printer");
await assert.rejects(printWat(Uint8Array.of(1)));
assert.equal(await printWat(cold.wasm), printed, "printing errors do not poison later requests");

// The shared host must be unused by entirely ASCII source and paths.
let nfcCalls = 0;
let nfcMemory;
const imports = compilerImports(() => {
  nfcCalls += 1;
  return nfcMemory;
});
const nfcInstance = (await WebAssembly.instantiate(readFileSync(COMPILER_WASM), imports)).instance;
nfcMemory = nfcInstance.exports.memory;
const checkFiles = (files) =>
  compile(nfcInstance.exports, encodeRequest(files, "src/lib.hd", GOAL.check));
assert.equal(checkFiles({ "src/lib.hd": "pub fn value() -> i32: +1\n" }).status, STATUS.ok);
assert.equal(nfcCalls, 0, "ASCII checking never calls normalization");
assert.equal(checkFiles({ "src/lib.hd": "pub fn café() -> i32: +1\n" }).status, STATUS.ok);
assert.ok(nfcCalls > 0);
for (const name of ["cafe\u0301", "`cafe\u0301`"]) {
  const result = checkFiles({ "src/lib.hd": `pub fn ${name}() -> i32: +1\n` });
  assert.equal(result.status, STATUS.errors);
  assert.ok(result.json.diagnostics.some(({ code }) => code === "invalid-token"));
}
// In-memory sources keep the host filesystem from merging distinct names.
for (const names of [
  ["Cart", "cart"],
  ["ŉ", "ʼn"],
  ["café", "cafe\u0301"],
]) {
  const result = checkFiles({
    "src/lib.hd": "",
    [`src/${names[0]}.hd`]: "pub fn first() -> i32: +1\n",
    [`src/${names[1]}.hd`]: "pub fn second() -> i32: +2\n",
  });
  assert.equal(result.status, STATUS.errors);
  assert.ok(result.json.diagnostics.some(({ code }) => code === "duplicate-module-name"));
  if (names[1] === "cafe\u0301") {
    assert.ok(result.json.diagnostics.some(({ code }) => code === "invalid-module-path"));
  }
}
console.log("NFC: ASCII host calls 0; identifiers and case-fold collisions passed");

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

// A REPL session: two inputs, each built as the session program and run
// in the JS host's session after replaying the inputs before it, then
// reported as joined.
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
const start = compile(exports, encodeReplStart({ "src/lib.hd": "fn base() -> i32: +40\n" }));
assert.equal(start.status, STATUS.ok, JSON.stringify(start.json));
assert.deepEqual(start.json, { diagnostics: [], input: 0 });
assert.equal(start.wasm.length, 0);
const sources = [];
let built = 0;
let firstMs = 0;
for (const [n, text] of [
  [1, "n := base() + 2\n"],
  [2, "n * 2\n"],
]) {
  const input = compile(exports, encodeReplInput(text));
  assert.equal(input.status, STATUS.ok, JSON.stringify(input.json));
  assert.equal(input.json.input, n);
  assert.deepEqual([...input.wasm.subarray(0, 4)], wasmMagic);
  built = input.wasm.length;
  firstMs ||= input.ms;
  sources.push([n, text]);
  const ran = await session.run(input.wasm, sources, n);
  assert.equal(ran.live.joined, true, replOut);
  const outcome = compile(exports, encodeReplOutcome(n, OUTCOME.joined));
  assert.deepEqual(outcome.json, { diagnostics: [], input: n, outcome: "joined" });
}
assert.equal(replOut, "84 : i32\n");
session.close();
const redeclared = compile(exports, encodeReplInput("n := 1\n"));
assert.equal(redeclared.status, STATUS.errors);
assert.equal(redeclared.json.diagnostics[0].path, "repl#3");
assert.equal(redeclared.wasm.length, 0);
const early = compile(exports, encodeReplOutcome(9, OUTCOME.joined));
assert.equal(early.status, STATUS.malformed);
console.log(`repl: session program ${built} B, first input cold ${firstMs.toFixed(0)} ms`);

assert.ok(watSize.gzip <= WAT_GZIP_LIMIT, `WAT gzip ${watSize.gzip} is over ${WAT_GZIP_LIMIT}`);
assert.ok(gzip <= GZIP_LIMIT, `gzip ${formatBytes(gzip)} is over ${formatBytes(GZIP_LIMIT)}`);
assert.ok(
  raw >= RAW_FLOOR,
  `raw ${formatBytes(raw)} is under ${formatBytes(RAW_FLOOR)}: LTO shrink`,
);
console.log("web compiler smoke test passed");
