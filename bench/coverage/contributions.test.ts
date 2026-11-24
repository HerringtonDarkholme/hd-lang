import assert from "node:assert/strict";
import { test } from "node:test";
import { contributions } from "./contributions.ts";

test("a redundant mode has no unique line even when it covers many lines", () => {
  const sets = new Map([
    ["core", new Set(["a:1", "a:2", "a:3"])],
    ["checked", new Set(["a:2", "a:3", "a:4"])],
    ["optimized", new Set(["a:2", "a:3"])],
  ]);
  assert.deepEqual(
    [...contributions(sets)],
    [
      ["core", 1],
      ["checked", 1],
      ["optimized", 0],
    ],
  );
});

test("all overlapping sets report zero, independent of order, with an empty set", () => {
  const shared = new Set(Array.from({ length: 1000 }, (_, index) => `a:${index}`));
  assert.deepEqual(
    [
      ...contributions(
        new Map([
          ["one", shared],
          ["two", shared],
          ["empty", new Set()],
        ]),
      ),
    ],
    [
      ["one", 0],
      ["two", 0],
      ["empty", 0],
    ],
  );
  assert.deepEqual(
    [
      ...contributions(
        new Map([
          ["two", shared],
          ["one", shared],
        ]),
      ),
    ],
    [
      ["two", 0],
      ["one", 0],
    ],
  );
});
