import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { hd } from "./hd-in-process.ts";

// The dogfood programs in examples/dogfood/ are small useful programs written
// against the current compiler (audit/dogfood-199.md). Each must keep passing
// `hd check`, and `hd test` when it has tests.

const root = resolve(import.meta.dirname, "..");
const directory = resolve(root, "examples/dogfood");

for (const file of readdirSync(directory).filter((name) => name.endsWith(".hd"))) {
  const path = resolve(directory, file);

  test(`dogfood program checks: ${file}`, async () => {
    const { stdout } = await hd(["check", path], { cwd: root });
    assert.match(stdout, /: ok$/m);
  });

  if (/^tests:/m.test(readFileSync(path, "utf8")))
    test(`dogfood tests pass: ${file}`, async () => {
      const { stdout } = await hd(["test", path], { cwd: root });
      assert.match(stdout, /: \d+ passed$/m);
    });
}
