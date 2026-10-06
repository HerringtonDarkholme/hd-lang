import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

// The fix hints that the 2026-10-06 usability probes asked for
// (audit/hd-writing-log.md): each has a case that names the fix, and a case
// where nothing is close enough, so the message stays bare.

function message(source: string, code: string): string {
  const found = analyze(source).diagnostics.filter((diagnostic) => diagnostic.code === code);
  assert.equal(found.length, 1, JSON.stringify(analyze(source).diagnostics));
  return found[0]!.message;
}

test("unknown-method suggests the hd name of another language's method", () => {
  assert.equal(
    message(
      `pub fn main() -> void:
    let words: mut List[string] = []
    words.append("a")
`,
      "unknown-method",
    ),
    "type 'mut List[string]' has no supported method 'append'; did you mean 'push'?",
  );
});

test("unknown-method suggests nothing for a name close to no method", () => {
  assert.equal(
    message(
      `pub fn main() -> void:
    let words: mut List[string] = []
    words.frobnicate("a")
`,
      "unknown-method",
    ),
    "type 'mut List[string]' has no supported method 'frobnicate'",
  );
});

test("unknown-name names the std module that exports a name", () => {
  assert.equal(
    message(
      `fn add(a: i32, b: i32) -> i32:
    a + b

tests:
    it("adds"):
        assert_equal(add(1, 2), +3, reason="sum")
`,
      "unknown-name",
    ),
    "unknown function 'assert_equal'; import it with use std.testing.assert_equal",
  );
});

test("unknown-name suggests a visible name it misspells", () => {
  assert.equal(
    message(
      `fn total() -> i32:
    let count = +0
    cout
`,
      "unknown-name",
    ),
    "unknown name 'cout'; did you mean 'count'?",
  );
});

test("unknown-name suggests nothing when no name is close", () => {
  assert.equal(
    message(
      `fn total() -> i32:
    let count = +0
    quizzical(count)
`,
      "unknown-name",
    ),
    "unknown function 'quizzical'",
  );
});

test("unknown-type and unknown-trait name the std module that exports them", () => {
  assert.equal(
    message("fn f(p: Path) -> void: pass\n", "unknown-type"),
    "unknown or unsupported type 'Path'; import it with use std.path.Path",
  );
  assert.equal(
    message("fn f() -> i64 $ Random:\n    +1\n", "unknown-trait"),
    "unknown trait 'Random' in requirement key 'Random'; import it with use std.random.Random",
  );
  assert.equal(
    message("fn f(p: Quizzical) -> void: pass\n", "unknown-type"),
    "unknown or unsupported type 'Quizzical'",
  );
});

test("mutable-receiver-required names the let form for a mutable initializer", () => {
  assert.equal(
    message(
      `use std.random.{Random, rng}

fn roll() -> i64 $ Random:
    draws := rng()
    draws.int(1..=6)
`,
      "mutable-receiver-required",
    ),
    "method 'int' takes mut self, but binding 'draws' has readonly type 'Rng'; declare it 'let mut draws = ...'",
  );
  assert.equal(
    message(
      `fn fill() -> void:
    let names: List[string] = []
    names.push("a")
`,
      "mutable-receiver-required",
    ),
    "method 'push' takes mut self, but binding 'names' has readonly type 'List[string]'; declare it 'let names: mut List[string] = ...'",
  );
});

test("mutable-receiver-required names no let form for a readonly initializer", () => {
  assert.equal(
    message(
      `fn fill(source: List[string]) -> void:
    let names = source
    names.push("a")
`,
      "mutable-receiver-required",
    ),
    "method 'push' takes mut self, but binding 'names' has readonly type 'List[string]'",
  );
});

test("iteration over a readonly iterator parameter names the parameter type", () => {
  assert.equal(
    message(
      `fn drain(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]
`,
      "mutable-receiver-required",
    ),
    "iteration requires mutable access to an Iterator implementation; declare the parameter 'source: mut Iterator[i32]'",
  );
});

test("unknown-variant suggests a close variant, or lists the variants", () => {
  assert.equal(
    message(
      `enum Color:
    Red
    Green

fn g(c: Color) -> i32:
    match c:
        .Gren => +1
        _ => +0
`,
      "unknown-variant",
    ),
    "enum 'Color' has no variant 'Gren'; did you mean 'Green'?",
  );
  assert.equal(
    message(
      `fn parse() -> Result[i32, string]:
    .Ok(+1)

fn f() -> i32:
    match parse():
        .Some(v) => v
        _ => +0
`,
      "unknown-variant",
    ),
    "enum 'Result' has no variant 'Some'; its variants are 'Ok', 'Err'",
  );
});

test("type-used-as-value names the provider of a requirement key", () => {
  assert.equal(
    message(
      `use std.time.{Clock}

fn f() -> void $ Clock:
    _ := Clock.now()
`,
      "type-used-as-value",
    ),
    "'Clock' names a type, not a value; to call its provider, write '$.use(Clock)'",
  );
  assert.equal(
    message(
      `data Point:
    x: i32

fn f() -> void:
    _ := Point.x
`,
      "type-used-as-value",
    ),
    "'Point' names a type, not a value",
  );
});

test("a use of a name with '!' says to drop it", () => {
  assert.equal(
    message("use std.fs.{FsRead, read_text!}\n", "syntax-error"),
    "expected '}', found '!'; a use names 'read_text' without '!', and a call writes 'read_text!(...)'",
  );
});

test("a variant pattern with type arguments says to drop them", () => {
  assert.equal(
    message(
      `enum Expr[T]:
    IntLit(value: i64) -> Expr[i64]

fn eval[T](expr: Expr[T]) -> T:
    match expr:
        Expr[i64].IntLit(value) => value
`,
      "syntax-error",
    ),
    "expected '=>', found '['; a variant pattern takes no type arguments, as in 'Enum.Variant(...)'",
  );
});
