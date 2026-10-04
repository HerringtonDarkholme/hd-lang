import assert from "node:assert/strict";
import fs from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { main } from "../src/cli.ts";
import { bufferedIo } from "../src/commands/io.ts";
import { analyze } from "../src/compiler.ts";
import { linkPackage } from "../src/package.ts";

const BROKEN_DIGEST = `pub fn broken() -> i32:
    "wrong"
`;

test("checker diagnostics inside std name their physical source", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "hd-std-location-"));
  const file = join(directory, "probe.hd");
  const source = `# first user line
# second user line
use std.digest.broken
pub fn main() -> void: pass
`;
  await writeFile(file, source);

  const original = fs.readFileSync;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) =>
    String(path).endsWith("/lib/std/digest.hd")
      ? BROKEN_DIGEST
      : Reflect.apply(original, fs, [path, ...arguments_])) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const text = bufferedIo();
    assert.equal(await main(["check", file], text), 1);
    assert.equal(
      text.output().stderr,
      "lib/std/digest.hd:2:5: type-mismatch: expected i32, found string\n",
    );

    const json = bufferedIo();
    assert.equal(await main(["check", "--format", "json", file], json), 1);
    const [first] = json.output().stdout.split("\n");
    const diagnostic = JSON.parse(first!) as {
      readonly file: string;
      readonly line: number;
      readonly column: number;
    };
    assert.equal(diagnostic.file, "lib/std/digest.hd");
    assert.deepEqual({ line: diagnostic.line, column: diagnostic.column }, { line: 2, column: 5 });

    const linked = linkPackage({ "src/main.hd": source }, "src/main.hd");
    assert.ok(linked.source);
    const analysis = analyze(linked.source, {
      parse: { joinedModules: true, initGroupStarts: linked.initGroups },
    });
    const located = linked.locate(analysis.diagnostics[0]!);
    assert.equal(located.path, "lib/std/digest.hd");
    assert.equal(located.span.start.line, 2);
    assert.equal(located.span.start.column, 5);
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
    await rm(directory, { recursive: true });
  }
});
