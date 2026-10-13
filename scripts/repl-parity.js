// The Node half of the cross-engine REPL session test (repl-plan.md job
// W4), which hd_run_wasmtime's `a_session_runs_and_replays_the_same_on_node`
// runs: `node scripts/repl-parity.js <dir>`. The directory holds a session
// that ran on wasmtime: `inputs` (the input numbers, in order), and for
// each input `N.hd`, `N.wasm`, `N.status`, `N.out` and `N.err` (its text,
// module, status and output); `wasmtime.journal`, the session's journal
// after them; and `after`, the number of one more input that ran after the
// journal was taken.
//
// 1. Runs the inputs on Node with the JS host's session (host/core.mjs)
//    and checks each one's status and output against wasmtime's, then
//    writes its journal as `node.journal` for the Rust test to replay.
// 2. Rebuilds a new Node session from the wasmtime journal: every input is
//    kept, no host call is made, nothing is printed, and the new journal
//    equals the wasmtime one byte for byte. Then the `after` input runs
//    live over the rebuilt bindings, with wasmtime's output.

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { createNodeSession } from "../host/node.mjs";

const dir = process.argv[2];
const read = (name) => readFileSync(join(dir, name), "utf8");
const input = (number) => ({
  number,
  source: read(`${number}.hd`),
  wasm: new Uint8Array(readFileSync(join(dir, `${number}.wasm`))),
  status: Number(read(`${number}.status`)),
  out: read(`${number}.out`),
  err: read(`${number}.err`),
});
const inputs = read("inputs").trim().split("\n").map(Number).map(input);
const after = input(Number(read("after")));

let out = "";
let err = "";
const sink = {
  out: (text) => {
    out += text;
  },
  err: (text) => {
    err += text;
  },
};
// Runs `i` in `session` and checks its status and output.
async function runs(session, i) {
  const ran = await session.input(i.number, i.source, i.wasm);
  assert.deepEqual(
    { input: i.number, status: ran.status, out, err, error: ran.error },
    { input: i.number, status: i.status, out: i.out, err: i.err, error: null },
  );
  out = "";
  err = "";
}

// The offset of the first byte where `a` and `b` differ, or -1.
const differs = (a, b) => {
  for (let i = 0; i < Math.max(a.length, b.length); i++) if (a[i] !== b[i]) return i;
  return -1;
};

const live = createNodeSession(sink, {});
for (const i of inputs) await runs(live, i);
writeFileSync(join(dir, "node.journal"), live.journal());
live.close();

const journal = new Uint8Array(readFileSync(join(dir, "wasmtime.journal")));
const byNumber = new Map(inputs.map((i) => [i.number, i]));
const again = createNodeSession(sink, {});
const rebuilt = await again.rebuild({ build: (n) => byNumber.get(n) }, journal);
assert.deepEqual(rebuilt, {
  kept: inputs.map((i) => i.number),
  skipped: [],
  dropped: [],
  reason: null,
});
assert.equal(again.hostCalls(), 0, "a host call repeated");
assert.equal(out + err, "", "the rebuild printed");
const at = differs(again.journal(), journal);
assert.equal(at, -1, `the rebuilt journal differs from the wasmtime journal at byte ${at}`);
await runs(again, after);
again.close();
console.log(`repl parity: ${inputs.length} inputs, journal ${journal.length} bytes`);
