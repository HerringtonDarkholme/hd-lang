import assert from "node:assert/strict";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { physicalSpan, sourceDocument } from "../src/diagnostics.ts";

test("standard template diagnostics retain their physical source", (t) => {
  const original = fs.readFileSync;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) => {
    const source = Reflect.apply(original, fs, [path, ...arguments_]);
    return String(path).endsWith("/lib/std/cmp.hd")
      ? String(source).replace(".Ok(_) => true", '.Ok(_) => "wrong"')
      : source;
  }) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const result = analyze("@derive(Eq)\ndata P:\n    x: i32\n");
    const diagnostic = result.diagnostics.find(({ code }) => code === "no-common-type");
    assert.ok(diagnostic);
    assert.equal(diagnostic.message, "match arms have no common type: string, bool");
    assert.equal(sourceDocument(diagnostic.span)?.file, "lib/std/cmp.hd");
    const start = physicalSpan(diagnostic.span).start;
    assert.deepEqual(
      { offset: start.offset, line: start.line, column: start.column },
      { offset: 8814, line: 267, column: 9 },
    );
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
  }
});
