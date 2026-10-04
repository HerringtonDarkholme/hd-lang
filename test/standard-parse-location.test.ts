import assert from "node:assert/strict";
import fs from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { main } from "../src/cli.ts";
import { bufferedIo } from "../src/commands/io.ts";

const MALFORMED_FS = "pub fn broken( -> i32: pass\n";

test("parser diagnostics inside std name their physical source", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "hd-std-parse-location-"));
  const file = join(directory, "probe.hd");
  await writeFile(file, "use std.fs.broken\npub fn main() -> void: pass\n");

  const original = fs.readFileSync;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) =>
    String(path).endsWith("/lib/std/fs.hd")
      ? MALFORMED_FS
      : Reflect.apply(original, fs, [path, ...arguments_])) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const output = bufferedIo();
    assert.equal(await main(["check", file], output), 1);
    assert.match(output.output().stderr, /^lib\/std\/fs\.hd:1:\d+: /);
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
    await rm(directory, { recursive: true });
  }
});
