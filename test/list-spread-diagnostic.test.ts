import assert from "node:assert/strict";
import test from "node:test";

import { parse } from "../src/parser/index.ts";

const MESSAGE = "a list spread is written with a suffix '...', as in '[xs...]'";

test("prefix list spreads suggest the postfix spelling", () => {
  for (const expression of ["[...xs]", "[0, ...xs]"]) {
    const diagnostics = parse(`value := ${expression}\n`).diagnostics;
    assert.equal(diagnostics.length, 1);
    assert.equal(diagnostics[0]?.code, "syntax-error");
    assert.equal(diagnostics[0]?.message, MESSAGE);
  }
});
