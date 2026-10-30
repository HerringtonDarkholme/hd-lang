// The Node half of the cross-engine REPL session test, which
// hd_run_wasmtime's `a_session_runs_and_replays_the_same_on_node` runs:
// `node scripts/repl-parity.js <dir>`. The directory holds a session that
// ran on wasmtime: `inputs` (the input numbers, in order), and for each
// input `N.hd`, `N.wasm`, `N.status`, `N.out` and `N.err` (its text, the
// session program it ran in, its status and output); `wasmtime.journal`,
// the session's journal after them; and `after`, the number of one more
// input that ran after the journal was taken.
//
// 1. Runs the inputs on Node with the JS host's session (host/core.ts),
//    each from its session program after replaying the ones before, and
//    checks each one's status and output against wasmtime's, then writes
//    its journal as `node.journal` for the Rust test to replay.
// 2. Rebuilds a new Node session from the wasmtime journal: every input is
//    kept, no host call is made, nothing is printed, and the new journal
//    equals the wasmtime one but for module hashes. Then the `after` input
//    runs live over the rebuilt bindings, with wasmtime's output.

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { recorded } from "../host/journal.ts";
import { createNodeSession } from "../host/node-session.ts";

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
// The texts of the inputs that ran so far, as `run` takes them.
const sources = [];
// Runs `i` in `session` after the inputs in `sources` and checks its
// status and output.
async function runs(session, i) {
  const ran = await session.run(i.wasm, [...sources, [i.number, i.source]], i.number);
  assert.deepEqual(
    { input: i.number, dropped: ran.dropped, status: ran.live?.status, out, err },
    { input: i.number, dropped: [], status: i.status, out: i.out, err: i.err },
  );
  out = "";
  err = "";
}

// What a journal records, with each input's module hash left out.
const butModules = (journal) => {
  const record = recorded(journal);
  for (const input of record.inputs) {
    input.module = 0n;
  }
  return record;
};

const live = createNodeSession(sink, {});
for (const i of inputs) {
  await runs(live, i);
  sources.push([i.number, i.source]);
}
writeFileSync(join(dir, "node.journal"), live.journal());
live.close();

const journal = new Uint8Array(readFileSync(join(dir, "wasmtime.journal")));
const again = createNodeSession(sink, {});
const rebuilt = await again.run(after.wasm, sources, null, journal);
assert.deepEqual(rebuilt, {
  kept: inputs.map((i) => i.number),
  skipped: [],
  dropped: [],
  reason: null,
  live: null,
});
assert.equal(again.hostCalls(), 0, "a host call repeated");
assert.equal(out + err, "", "the rebuild printed");
assert.deepEqual(butModules(again.journal()), butModules(journal));
await runs(again, after);
again.close();
console.log(`repl parity: ${inputs.length} inputs, journal ${journal.length} bytes`);
