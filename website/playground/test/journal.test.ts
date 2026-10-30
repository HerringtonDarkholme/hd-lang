// The JS host's journal (host/journal.mts) against the bytes `hd_run`
// writes: the same xxh3-128 vectors and the same journal for the same
// recorder calls as crates/hd_run/src/journal_tests.rs pins
// (`the_js_host_checks_the_same_bytes`), the torn-tail reader, and the
// replay's matching rules and divergence messages (replay.rs).

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  Diverged,
  Recorder,
  Replay,
  callArgs,
  crc32,
  decodeRows,
  readRows,
  recorded,
  joinJournals,
  requestDigest,
  xxh3_128,
  type Value,
} from "../../../host/journal.mts";

const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");
const text = (s: string) => new TextEncoder().encode(s);
const int = (v: bigint | number): Value => ({ tag: 1, v: BigInt(v) });

// journal_tests.rs `SHARED_JOURNAL`.
const SHARED_JOURNAL = [
  "000011010700000000000000000000000000000042427bb4010121016ff4",
  "9b92aa48a5e2dd754a768d92cd38655451d8e889b2b73d5204d3aa0ec02d",
  "870f2d11020216000868643a436c6f636b036e6f77000180d095ffbc31ea",
  "9a847a020324010a68643a436f6e736f6c650b77726974652e7374617274",
  "1ff9808ebc2a9dd86d0301096ed099da02040d000001ffffffffffffffff",
  "ff01ffa078210505020109d4d55f5a040606020209808040a50630f80307",
  "05090501010082bed05c0608010420244a1407090102478c5744080a0103",
  "de127569010b2102db1edfbd0872f61b8e9b61920d252825655451d8e889",
  "b2b73d5204d3aa0ec02dd6a29313020c2a020768643a7072696d0a666f72",
  "6d61745f66363408000000000000044006000000000000e03f03302e35e9",
  "c75caf080d0200006fb0bc07",
].join("");

// journal_tests.rs `SHARED_XXH3`: a prefix of each length class.
const SHARED_XXH3: [number, string][] = [
  [0, "99aa06d3014798d86001c324468d497f"],
  [1, "a6cd5e9392000f6ac44bdff4074eecdb"],
  [2, "e3e3af28e278815150a0dc43c774eb84"],
  [3, "4af3603b5bd30dfe3698b80191e625f9"],
  [4, "11527735d62fce43eecdfa5a3b083ba8"],
  [7, "969f5114845f170c0624d295fe95cc20"],
  [8, "791e22cdc30880ba073a25812fe4f600"],
  [9, "22775b5e55c2ace94557529981ebbef0"],
  [16, "4ad789329416e4387df51ed58f26bee2"],
  [17, "950dda39877c26d1ff1aad989e0dd9f2"],
  [32, "ca5f929f445a49b8b88ffeebd235e209"],
  [33, "a9f9da0aaab1a6d38b0cd55037ee0dea"],
  [64, "3a648b5f5e597066a09efb35b93b4dd4"],
  [65, "10723f3d271e6ae65aa37a76af0a6bc0"],
  [96, "ecaf417d05d3e773d6e1529784e4567e"],
  [97, "64f1cf1beb24c0d4ea2789b946d00e9d"],
  [128, "2420677499e7498929b103324f4df782"],
  [129, "385edf5918e02b86a1a1b3d61e7a4029"],
  [200, "cab4b6c1d7f66c432a1a7281f2883dff"],
  [240, "7cf77784002208a5423167d7599e74a5"],
  [241, "095d0994f2a5191d82d3ebf50a3e9c42"],
  [1024, "5f6ea7ec3ae785ebcbdff56b050f2c1c"],
  [1025, "4e0bf1adc121b7df9babf50235cea0a0"],
  [2048, "3af1daea3bdb2821eef46c3d948c544e"],
  [2100, "99f2fb8295d819cdc04609bcfd9a9cd2"],
];

/** The execution id 7, as 16 little-endian bytes. */
const seven = () => {
  const id = new Uint8Array(16);
  id[0] = 7;
  return id;
};

/** journal_tests.rs `every_row_kind`, through the JS recorder. */
function everyRowKind(): Recorder {
  const r = new Recorder(1 << 20, seven());
  r.input(1, "now()\n", text("\0asm"));
  r.call("hd:Clock", "now", new Uint8Array(), int(1_700_000_000_000n), null);
  const line = callArgs([5], ["i32"], text("a line longer than eight bytes"));
  r.call("hd:Console", "write.start", line, { tag: 3, v: 1, w: 9 }, null);
  r.call("hd:Clock", "now", new Uint8Array(), int(-1), null);
  r.block([9]);
  r.poll();
  r.poll();
  r.wake([9, 1 << 20]);
  r.finish(9, int(1), Uint8Array.of(0));
  r.abort(4);
  r.cancel();
  r.end({ kind: "cancelled" });
  r.input(2, "x\n", text("\0asm"));
  const half = callArgs([2.5], ["f64"], null);
  const bits = new DataView(new Float64Array([0.5]).buffer).getBigUint64(0, true);
  r.call("hd:prim", "format_f64", half, { tag: 2, v: bits }, text("0.5"));
  r.end({ kind: "exit", status: 0 });
  return r;
}

test("archived chunks join without losing method identities or recorded results", () => {
  const first = everyRowKind().journal()!;
  const second = new Recorder(1 << 20, seven());
  second.input(3, "later()", text("later module"));
  second.call("hd:prim", "format_f64", new Uint8Array(), int(17), text("retained reply"));
  second.call("hd:Clock", "now", text("a request longer than eight bytes"), int(18), null);
  second.end({ kind: "exit", status: 0 });
  const tail = second.journal()!;
  const empty = new Recorder(1 << 20, seven()).journal()!;
  assert.deepEqual(joinJournals([first]), first, "one chunk keeps its exact wire format");
  const joined = joinJournals([first, tail, empty]);
  assert.deepEqual(recorded(joined), {
    execution: seven(),
    inputs: [...recorded(first).inputs, ...recorded(tail).inputs],
  });
  assert.equal(readRows(joined).valid, joined.length);
  assert.deepEqual(
    readRows(joined).rows.map((row) => row.index),
    readRows(joined).rows.map((_, i) => i),
  );
  const copy = first.slice();
  joinJournals([first, tail]);
  assert.deepEqual(first, copy, "joining leaves the original archive intact");
});

test("archive joining rejects incomplete, foreign and reordered history", () => {
  const first = everyRowKind().journal()!;
  const foreign = new Recorder(1 << 20, new Uint8Array(16)).journal()!;
  assert.throws(() => joinJournals([first, foreign]), /different executions/);
  assert.throws(() => joinJournals([first, first]), /increasing order/);
  assert.throws(() => joinJournals([first.subarray(0, first.length - 1)]), /cut short/);
  const pending = new Recorder(1 << 20, seven());
  pending.input(3, "busy()", text("busy module"));
  assert.throws(() => joinJournals([first, pending.journal()!]), /unfinished input/);
});

test("xxh3-128 matches hd_base::hash128 at every length class", () => {
  const bytes = Uint8Array.from({ length: 2100 }, (_, i) => (i * 31) % 251);
  for (const [len, hash] of SHARED_XXH3) {
    assert.equal(xxh3_128(bytes.subarray(0, len)).toString(16).padStart(32, "0"), hash, `${len}`);
  }
});

test("the recorder writes the bytes hd_run's recorder writes", () => {
  const journal = everyRowKind().journal()!;
  assert.equal(hex(journal), SHARED_JOURNAL);
  const events = decodeRows(readRows(journal).rows);
  const kinds = "header input call call call block wake finish abort cancel end input call end";
  assert.deepEqual(
    events.map((e) => e.kind),
    kinds.split(" "),
  );
  const long = text("a line longer than eight bytes");
  assert.deepEqual(events[3], {
    kind: "call",
    module: "hd:Console",
    name: "write.start",
    request: { len: 31, digest: requestDigest(Uint8Array.of(5, ...long)) },
    value: { tag: 3, v: 1, w: 9 },
    result: null,
  });
  assert.deepEqual(events[4], {
    kind: "call",
    module: "hd:Clock",
    name: "now",
    request: { args: new Uint8Array() },
    value: int(-1),
    result: null,
  });
});

test("the reader cuts a torn tail and checks each row's checksum", () => {
  assert.equal(crc32(text("123456789")), 0xcbf43926);
  assert.equal(crc32(new Uint8Array()), 0);
  const journal = everyRowKind().journal()!;
  assert.equal(readRows(journal).valid, journal.length);
  const torn = readRows(journal.subarray(0, journal.length - 2));
  assert.equal(torn.rows.length, 13);
  const flipped = journal.slice();
  flipped[40] ^= 1;
  assert.ok(readRows(flipped).rows.length < 3);
  const record = recorded(journal.subarray(0, journal.length - 3));
  assert.deepEqual(
    record.inputs.map((i) => i.end),
    [{ kind: "cancelled" }, null],
  );
});

/** replay_tests.rs `journal`: a clock reading and a written line, then a
 * stopped input. */
function replayed() {
  const r = new Recorder(1 << 20, seven());
  r.input(1, "a := now()\n", text("\0asm1"));
  r.call("hd:Clock", "now", new Uint8Array(), int(1_700), null);
  r.call("hd:Console", "write.start", Uint8Array.of(2, 0x68, 0x69), { tag: 3, v: 1, w: 4 }, null);
  r.poll();
  r.wake([4]);
  r.finish(4, int(1), Uint8Array.of(0));
  r.end({ kind: "exit", status: 0 });
  r.input(2, "while true: pass\n", text("\0asm2"));
  r.end({ kind: "stopped" });
  return recorded(r.journal()!);
}

test("a replay serves the recorded replies in order", () => {
  const record = replayed();
  assert.deepEqual(
    record.inputs.map((i) => [i.number, i.events.length, i.end]),
    [
      [1, 4, { kind: "exit", status: 0 }],
      [2, 0, { kind: "stopped" }],
    ],
  );
  const replay = new Replay(record.inputs[0]!);
  assert.deepEqual(replay.call("hd:Clock", "now", new Uint8Array()).value, int(1_700));
  const line = replay.call("hd:Console", "write.start", Uint8Array.of(2, 0x68, 0x69));
  assert.deepEqual(line.value, { tag: 3, v: 1, w: 4 });
  replay.poll();
  assert.ok(replay.wakeNext());
  assert.deepEqual(replay.wake(), [4]);
  assert.deepEqual(replay.finish(4).result, Uint8Array.of(0));
  replay.end({ kind: "exit", status: 0 }, record.inputs[0]!.end);
  assert.equal(replay.diverged, null);
});

test("a request that does not match diverges with replay.rs's message", () => {
  const input = replayed().inputs[0]!;
  const diverges = (run: (r: Replay) => unknown, message: string) => {
    const replay = new Replay(input);
    assert.throws(
      () => run(replay),
      (e: unknown) => e instanceof Diverged && e.message === message,
    );
    // The first divergence is the one kept.
    assert.throws(() => replay.end({ kind: "exit", status: 0 }, input.end), { message });
  };
  diverges(
    (r) => r.call("hd:Clock", "monotonic", new Uint8Array()),
    "input 1 called hd:Clock monotonic, but the record has a call of hd:Clock now",
  );
  diverges((r) => {
    r.call("hd:Clock", "now", new Uint8Array());
    r.call("hd:Console", "write.start", Uint8Array.of(2, 0x68, 0x6f));
  }, "input 1 called hd:Console write.start with other arguments than before");
  diverges((r) => {
    r.call("hd:Clock", "now", new Uint8Array());
    r.call("hd:Console", "write.start", Uint8Array.of(2, 0x68, 0x69));
    r.wake();
  }, "input 1 waited on the host after 0 polls, but the record has a wake after 1 polls");
  diverges(
    (r) => r.end({ kind: "trapped" }, input.end),
    "input 1 panicked early, but the record has a call of hd:Clock now",
  );
  diverges((r) => {
    r.call("hd:Clock", "now", new Uint8Array());
    r.call("hd:Console", "write.start", Uint8Array.of(2, 0x68, 0x69));
    r.poll();
    r.wake();
    r.finish(4);
    r.end({ kind: "exit", status: 2 }, input.end);
  }, "input 1 ended with status 2, but before it ended");
});
