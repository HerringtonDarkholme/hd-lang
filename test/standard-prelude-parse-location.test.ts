import assert from "node:assert/strict";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import test from "node:test";

import { physicalSpan, sourceDocument } from "../src/diagnostics.ts";

test("prelude parser diagnostics retain their physical source", async (t) => {
  const original = fs.readFileSync;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) =>
    String(path).endsWith("/lib/std/prelude.hd")
      ? "pub use std.cmp.{Eq\n"
      : Reflect.apply(original, fs, [path, ...arguments_])) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const { analyze } = await import("../src/compiler.ts");
    const result = analyze("pub fn main() -> void: pass\n");
    const diagnostic = result.diagnostics[0];
    assert.ok(diagnostic);
    assert.equal(sourceDocument(diagnostic.span)?.file, "lib/std/prelude.hd");
    assert.equal(physicalSpan(diagnostic.span).start.line, 1);
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
  }
});
