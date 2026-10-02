import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

import { createAdapter } from "./hd-adapter.ts";

const root = resolve(import.meta.dirname, "..");
const directory = mkdtempSync(join(tmpdir(), "hd-adapter-"));

function fixture(name: string, source: string): string {
  const path = join(directory, name);
  writeFileSync(path, source);
  return path;
}

/** What `node bin/hd.js ARGS...` reports, for comparison. */
async function spawned(args: readonly string[]) {
  try {
    const { stdout, stderr } = await promisify(execFile)(
      process.execPath,
      [resolve(root, "bin/hd.js"), ...args],
      { cwd: root, encoding: "utf8" },
    );
    return { status: 0, stdout, stderr };
  } catch (error) {
    const failed = error as { code: number; stdout: string; stderr: string };
    return { status: failed.code, stdout: failed.stdout, stderr: failed.stderr };
  }
}

test("the in-process adapter reports what the spawned CLI reports", async () => {
  const adapter = createAdapter({ jobs: 2 });
  try {
    const printing = fixture(
      "printing.hd",
      'pub fn main() -> void $ Console:\n    println("hi \\u{1F600}")\n',
    );
    const rejected = fixture(
      "rejected.hd",
      'pub fn main() -> i32:\n    let x: i32 = "no"\n    x\n',
    );
    const panicking = fixture(
      "panicking.hd",
      "fn divide(a: i32, b: i32) -> i32:\n    a / b\n\npub fn main() -> void:\n    let x = divide(1, 0)\n",
    );
    for (const args of [
      ["test", printing],
      ["run", printing],
      ["check", "--tests", rejected],
      ["run", panicking],
      ["test", panicking],
      ["run", "--entry", "main", printing],
      ["check", "--format", "json", rejected],
      ["bogus"],
      ["help", "test"],
    ]) {
      const { timedOut, ...result } = await adapter.run(args, 10_000);
      assert.equal(timedOut, false);
      assert.deepEqual(result, await spawned(args), args.join(" "));
    }
  } finally {
    await adapter.close();
  }
});

test("a case past the timeout stops, and its worker is replaced", async () => {
  const adapter = createAdapter({ jobs: 1 });
  try {
    const hung = fixture("hung.hd", "pub fn main() -> void:\n    while true:\n        pass\n");
    const quick = fixture("quick.hd", "pub fn main() -> void:\n    pass\n");
    const stopped = await adapter.run(["run", hung], 1_000);
    assert.equal(stopped.timedOut, true);
    assert.equal(stopped.status, null);
    const next = await adapter.run(["check", quick], 10_000);
    assert.deepEqual(next, {
      status: 0,
      stdout: `${quick}: ok\n`,
      stderr: "",
      timedOut: false,
    });
  } finally {
    await adapter.close();
  }
});
