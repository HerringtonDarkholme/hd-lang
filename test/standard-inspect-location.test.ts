import assert from "node:assert/strict";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { physicalSpan, sourceDocument } from "../src/diagnostics.ts";
import { stdLocation } from "./std-location.ts";

test("compiler-injected inspect diagnostics retain their physical source", (t) => {
  const expected = stdLocation(
    "inspect.hd",
    "fn eq(self, other: TypeId) -> bool: self.key == other.key",
    "self.key",
  );
  const original = fs.readFileSync;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) => {
    const source = Reflect.apply(original, fs, [path, ...arguments_]);
    return String(path).endsWith("/lib/std/inspect.hd")
      ? String(source).replace(
          "fn eq(self, other: TypeId) -> bool: self.key == other.key",
          'fn eq(self, other: TypeId) -> bool: "wrong"',
        )
      : source;
  }) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const result = analyze("use std.inspect.Inspectable\npub fn main() -> void: pass\n");
    const diagnostic = result.diagnostics.find(({ code }) => code === "type-mismatch");
    assert.ok(diagnostic);
    assert.equal(diagnostic.message, "expected bool, found string");
    assert.equal(sourceDocument(diagnostic.span)?.file, "lib/std/inspect.hd");
    const start = physicalSpan(diagnostic.span).start;
    assert.deepEqual({ offset: start.offset, line: start.line, column: start.column }, expected);
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
  }
});
