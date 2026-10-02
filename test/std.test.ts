import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { withStandardLibrary } from "../src/checker/standard-library.ts";
import { parse } from "../src/parser/index.ts";
import { STANDARD_MODULES, standardSource } from "../src/checker/standard-sources.ts";
import { hd } from "./hd-in-process.ts";

// The toy standard library in lib/std/: each test/std/*.hd file exercises
// one or two modules through `hd test`.

const root = resolve(import.meta.dirname, "..");
const directory = resolve(root, "test/std");

for (const file of readdirSync(directory).filter((name) => name.endsWith(".hd"))) {
  test(`std tests pass: ${file}`, async () => {
    const { stdout } = await hd(["test", resolve(directory, file)], { cwd: root });
    assert.match(stdout, /: \d+ passed/);
  });
}

test("every std module parses on its own", () => {
  for (const module of [...STANDARD_MODULES, "structure", "inspect"] as const) {
    const parsed = parse(standardSource(module), { standardLibrary: true });
    assert.deepEqual(
      parsed.diagnostics.map((diagnostic) => diagnostic.code),
      [],
      `std.${module} parses`,
    );
  }
});

test("a program that selects no std member is unchanged", () => {
  const program = parse("fn first(items: List[i32]) -> i32: items[0]\n").program!;
  assert.equal(withStandardLibrary(program), program);
});

test("a string method declares only the std helpers it reaches", () => {
  const program = parse('fn size() -> i32: "abc".len()\n').program!;
  const joined = withStandardLibrary(program);
  assert.deepEqual(joined.functions.map((declaration) => declaration.name).sort(), [
    "__std_text_byte_len",
    "size",
  ]);
  assert.deepEqual(joined.data, []);
  assert.equal(
    joined.functions.find((declaration) => declaration.name === "__std_text_byte_len")?.intrinsic,
    "string_byte_len",
  );
});

test("println is the std.console declaration, under its prelude name", () => {
  const program = parse('fn greet() -> void $ Console: println("hi")\n').program!;
  const println = withStandardLibrary(program).functions.find(
    (declaration) => declaration.name === "println",
  );
  assert.equal(println?.standard, true);
  const own = parse("fn println() -> void: pass\n").program!;
  assert.equal(withStandardLibrary(own), own);
});

test("only lib/std can declare an intrinsic", () => {
  // In user code the line is an ordinary decorator whose value calls an
  // unknown function (spec/lang/14-annotations.md#prefix-decorators).
  const analysis = analyze('@intrinsic("string_byte_len")\nfn size(text: string) -> i32: 0\n');
  assert.deepEqual(
    analysis.diagnostics.map((diagnostic) => diagnostic.code),
    ["unknown-name"],
  );
});

test("only the selected built-in methods are declared", () => {
  const program = parse("fn fallback(value: i32?) -> i32: value.unwrap_or(0)\n").program!;
  const joined = withStandardLibrary(program);
  const methods = joined.implementations.flatMap((implementation) =>
    implementation.standard ? implementation.methods.map((method) => method.name) : [],
  );
  // Selection is by name, so `Result.unwrap_or` comes along with `T?`'s.
  assert.deepEqual(methods, ["unwrap_or", "unwrap_or"]);
  assert.equal(joined.functions.length, program.functions.length);
});

test("an imported std name takes its local alias; the rest stay hidden", () => {
  const analysis = analyze(
    "use std.cmp.min as smaller\n\nfn least() -> i32: smaller(3, 2)\nfn other() -> i32: min(3, 2)\n",
  );
  assert.deepEqual(
    analysis.diagnostics.map((diagnostic) => diagnostic.code),
    ["unknown-name"],
  );
  assert.match(analysis.diagnostics[0]!.message, /min/);
});

test("user code cannot declare inherent methods on a built-in type", () => {
  const analysis = analyze("impl string:\n    pub fn shout(self) -> string: self\n");
  assert.deepEqual(
    analysis.diagnostics.map((diagnostic) => diagnostic.code),
    ["orphan-impl"],
  );
});
