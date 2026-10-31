import assert from "node:assert/strict";
import test from "node:test";

import {
  forgetState,
  loadState,
  MAX_SAVED_BYTES,
  MAX_SHARE_HASH,
  saveState,
  type SessionState,
  sessionFromHash,
  shareLink,
} from "../src/repl-state.ts";

const state = (overrides: Partial<SessionState> = {}): SessionState => ({
  files: { "src/main.hd": "pub fn main() -> void:\n    pass\n" },
  inputs: [
    [1, "x := 21"],
    [2, "x * 2"],
  ],
  next: 3,
  journal: Uint8Array.from([1, 2, 3, 250, 251, 252]),
  transcript: [
    { c: "repl-echo", t: "hd> x := 21", n: 1 },
    { c: "repl-out", t: "42 : usize" },
    { c: "repl-note", t: "restored: 2 inputs replayed" },
  ],
  ...overrides,
});

const KEPT = [
  { c: "repl-echo", t: "hd> x := 21", n: 1 },
  { c: "repl-out", t: "42 : usize" },
];

/** A just-enough IDBFactory: one database with one object store, in memory. */
function fakeIndexedDb(): IDBFactory {
  const data = new Map<string, unknown>();
  const done = <Result>(result: Result): IDBRequest<Result> => {
    const req = {} as IDBRequest<Result>;
    Object.defineProperty(req, "result", { get: () => result });
    setTimeout(() => (req as unknown as { onsuccess?: () => void }).onsuccess?.(), 0);
    return req;
  };
  const store = {
    put: (value: unknown, key: string) => (data.set(key, structuredClone(value)), done(key)),
    get: (key: string) => done(data.get(key)),
    delete: (key: string) => (data.delete(key), done(undefined)),
  };
  const db = {
    objectStoreNames: { contains: () => true },
    transaction: () => ({ objectStore: () => store }),
    close() {},
  };
  return {
    open() {
      return done(db);
    },
  } as unknown as IDBFactory;
}

test("a saved session loads back, and forgetting removes it", async () => {
  const idb = fakeIndexedDb();
  assert.equal(await saveState("k", state(), idb), "saved");
  const loaded = await loadState("k", idb);
  assert.deepEqual(loaded, { ...state(), transcript: KEPT.map((item) => ({ ...item })) });
  assert.equal(await loadState("other", idb), undefined);
  await forgetState("k", idb);
  assert.equal(await loadState("k", idb), undefined);
});

test("storage that is missing or throws just starts fresh", async () => {
  assert.equal(await saveState("k", state(), undefined), "unavailable");
  assert.equal(await loadState("k", undefined), undefined);
  const blocked = {
    open() {
      throw new DOMException("blocked", "SecurityError");
    },
  } as unknown as IDBFactory;
  assert.equal(await saveState("k", state(), blocked), "unavailable");
  assert.equal(await loadState("k", blocked), undefined);
  await forgetState("k", blocked);
  const failing = {
    open() {
      const req = {} as IDBOpenDBRequest;
      setTimeout(() => (req as unknown as { onerror?: () => void }).onerror?.(), 0);
      return req;
    },
  } as unknown as IDBFactory;
  assert.equal(await saveState("k", state(), failing), "unavailable");
  assert.equal(await loadState("k", failing), undefined);
});

test("a session past the size cap, or without a journal, is not saved", async () => {
  const idb = fakeIndexedDb();
  assert.equal(await saveState("k", state(), idb), "saved");
  const big = state({ journal: new Uint8Array(MAX_SAVED_BYTES + 1) });
  assert.equal(await saveState("k", big, idb), "too-big");
  assert.equal(await loadState("k", idb), undefined, "the stale save is dropped");
  assert.equal(await saveState("k", state({ journal: null }), idb), "too-big");
});

test("a damaged saved value loads as nothing", async () => {
  const idb = fakeIndexedDb();
  assert.equal(await loadState("k", idb), undefined);
});

test("a share link round-trips inputs, journal and transcript", async () => {
  const link = await shareLink(state());
  assert.equal(link.outputs, true);
  assert.match(link.hash, /^#repl=[zj]\.[A-Za-z0-9_-]+$/);
  const back = await sessionFromHash(link.hash);
  assert.deepEqual(back, { ...state(), transcript: KEPT.map((item) => ({ ...item })) });
  assert.deepEqual(await sessionFromHash(link.hash.slice(1)), back);
});

test("a session too long for a link shares its inputs only", async () => {
  // Random bytes do not compress, so the journal alone passes the limit.
  const noise = crypto.getRandomValues(new Uint8Array(MAX_SHARE_HASH));
  const link = await shareLink(state({ journal: noise }));
  assert.equal(link.outputs, false);
  assert.equal(link.long, false);
  const back = await sessionFromHash(link.hash);
  assert.equal(back?.journal, null);
  assert.deepEqual(back?.transcript, []);
  assert.deepEqual(back?.inputs, state().inputs);
  assert.deepEqual(back?.files, state().files);
});

test("malformed links open nothing", async () => {
  for (const hash of [
    "#repl=",
    "#repl=z.",
    "#repl=z.!!!",
    "#repl=q.AAAA",
    "#repl=z.AAAA",
    "#repl=j.e30",
    "#repl=j.bm90IGpzb24",
    "#code=abc",
  ]) {
    assert.equal(await sessionFromHash(hash), undefined, hash);
  }
});
