import assert from "node:assert/strict";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { physicalSpan, sourceDocument } from "../src/diagnostics.ts";

const STRUCTURE = new URL("../lib/std/structure.hd", import.meta.url);

test("compiler-injected structure diagnostics retain their physical source", (t) => {
  const original = fs.readFileSync;
  // Where the injected `"wrong"` lands, read from the file, so edits above
  // it (doc comments) don't move the expectation.
  const wrongOffset =
    String(original(STRUCTURE, "utf8")).indexOf("fn hd__no_default[HdF]() -> HdF?:\n    .None") +
    "fn hd__no_default[HdF]() -> HdF?:\n    ".length;
  const replacement = ((path: fs.PathOrFileDescriptor, ...arguments_: unknown[]) => {
    const source = Reflect.apply(original, fs, [path, ...arguments_]);
    return String(path).endsWith("/lib/std/structure.hd")
      ? String(source).replace(
          "fn hd__no_default[HdF]() -> HdF?:\n    .None",
          'fn hd__no_default[HdF]() -> HdF?:\n    "wrong"',
        )
      : source;
  }) as typeof fs.readFileSync;
  const mocked = t.mock.method(fs, "readFileSync", replacement);
  syncBuiltinESMExports();

  try {
    const result = analyze("@derive(Eq)\ndata P:\n    x: i32\n");
    const diagnostic = result.diagnostics.find(({ code }) => code === "type-mismatch");
    assert.ok(diagnostic);
    assert.equal(diagnostic.message, "expected HdF?, found string");
    assert.equal(sourceDocument(diagnostic.span)?.file, "lib/std/structure.hd");
    const start = physicalSpan(diagnostic.span).start;
    const before = String(original(STRUCTURE, "utf8")).slice(0, wrongOffset);
    assert.deepEqual(
      { offset: start.offset, line: start.line, column: start.column },
      {
        offset: wrongOffset,
        line: before.split("\n").length,
        column: wrongOffset - before.lastIndexOf("\n"),
      },
    );
  } finally {
    mocked.mock.restore();
    syncBuiltinESMExports();
  }
});
