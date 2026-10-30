import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createWatPrinter } from "../src/wat-printer.ts";

const empty = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0);
const bytes = () => readFile(new URL("../assets/hd_wat.wasm", import.meta.url));

test("WAT loads lazily and shares concurrent and repeated requests", async () => {
  let loads = 0;
  let notices = 0;
  let release!: () => void;
  const waiting = new Promise<void>((resolve) => {
    release = resolve;
  });
  const print = createWatPrinter(async () => {
    loads += 1;
    await waiting;
    return bytes();
  });
  assert.equal(loads, 0);
  const first = print(empty, () => {
    notices += 1;
  });
  const second = print(empty, () => {
    notices += 1;
  });
  assert.equal(loads, 1);
  assert.equal(notices, 1);
  release();
  assert.deepEqual(await Promise.all([first, second]), ["(module)\n", "(module)\n"]);
  assert.equal(await print(empty), "(module)\n");
  assert.equal(loads, 1);
  await assert.rejects(print(Uint8Array.of(1)));
  assert.equal(await print(empty), "(module)\n", "printing errors do not poison the instance");
});

test("a failed WAT download is reported and a later request can retry", async () => {
  let loads = 0;
  const print = createWatPrinter(async () => {
    loads += 1;
    if (loads === 1) {
      throw new Error("HTTP 503");
    }
    return bytes();
  });
  await assert.rejects(print(empty), /HTTP 503/);
  assert.equal(await print(empty), "(module)\n");
  assert.equal(loads, 2);
});
