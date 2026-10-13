// How the REPL panel reads inputs (src/repl-input.ts): continuation as
// `hd` continues an input at the terminal, and how a code block's Try in
// REPL splits into inputs.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  backspaceWidth,
  continuationIndent,
  needsMoreInput,
  splitInputs,
} from "../src/repl-input.ts";

test("an input continues while it is incomplete", () => {
  const more = [
    "fn f() -> i32:",
    "while true:  # spins",
    "[1,",
    "f(",
    'x := """',
    'x := """\nline',
    "fn f() -> i32:\n    1",
    "if a:\n    b\nelse:",
  ];
  for (const text of more) assert.equal(needsMoreInput(text), true, JSON.stringify(text));
  const done = [
    "[3, 6]",
    "fn f() -> i32: 1",
    'x := "a:"',
    'x := "("',
    "x := 1  # done:",
    'x := """a"""',
    "fn f() -> i32:\n    1\n",
    "fn f() -> i32:\n    1\n    ",
    "[1,\n2]",
  ];
  for (const text of done) assert.equal(needsMoreInput(text), false, JSON.stringify(text));
});

test("a continuation line indents one level after a header", () => {
  assert.equal(continuationIndent("while true:"), "    ");
  assert.equal(continuationIndent("    if x:  # yes"), "        ");
  assert.equal(continuationIndent("    pass"), "    ");
  assert.equal(backspaceWidth("        "), 4);
  assert.equal(backspaceWidth("      "), 2);
  assert.equal(backspaceWidth("    x"), 1);
  assert.equal(backspaceWidth(""), 1);
});

test("a code block splits into one input per top-level item", () => {
  const block = [
    "# Greets someone.",
    "@derive(Debug)",
    "data Point:",
    "    x: i32",
    "",
    "fn greet(name: string) -> string:",
    '    "hello, $name"',
    "",
    'greet("hd")',
    "if true:",
    "    1",
    "else:",
    "    2",
    "xs := [",
    "1,",
    "]",
    "# trailing",
  ].join("\n");
  assert.deepEqual(splitInputs(block), [
    "# Greets someone.\n@derive(Debug)\ndata Point:\n    x: i32\n",
    'fn greet(name: string) -> string:\n    "hello, $name"\n',
    'greet("hd")\n',
    "if true:\n    1\nelse:\n    2\n",
    "xs := [\n1,\n]\n",
  ]);
});
