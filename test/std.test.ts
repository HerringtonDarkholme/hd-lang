import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { standardModulesOf, withStandardLibrary } from "../src/checker/standard-library.ts";
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

// Every module joins the prelude's use graph (lib/std/prelude.hd).
const PRELUDE_GRAPH = [
  "annotation",
  "cmp",
  "convert",
  "format",
  "function",
  "hash",
  "collections",
  "console",
  "iter",
  "num",
  "ops",
  "option",
  "result",
  "task",
  "text",
  "prelude",
];

test("only a program with test code reaches std.testing through the prelude", () => {
  // spec/lang/10-modules.md#r-module.prelude.test-only
  const plain = parse("fn first(items: List[i32]) -> i32: items[0]\n").program!;
  assert.ok(!standardModulesOf(plain).includes("testing"));
  const tested = parse('fn one() -> i32: 1\n\ntests:\n    it("one"):\n        pass\n').program!;
  const modules = standardModulesOf(tested);
  for (const module of ["testing", "prelude.testing"] as const)
    assert.ok(modules.includes(module), module);
});

test("a program that uses no std module gets the prelude's use graph", () => {
  const program = parse("fn first(items: List[i32]) -> i32: items[0]\n").program!;
  assert.deepEqual(standardModulesOf(program), PRELUDE_GRAPH);
  const joined = withStandardLibrary(program);
  // String `+`, `==`, and order compile to calls of std.text's string kernel.
  for (const name of ["string_concat", "string_equal", "string_compare"])
    assert.ok(joined.functions.some((declaration) => declaration.name === `__std_text_${name}`));
  assert.equal(
    joined.functions.find((declaration) => declaration.name === "__std_text_bytes_len")?.intrinsic,
    "bytes_len",
  );
});

test("a use adds its module and the modules that module uses", () => {
  const program = parse("use std.error.Error\n").program!;
  assert.deepEqual(
    standardModulesOf(program),
    STANDARD_MODULES.filter((module) => module === "error" || PRELUDE_GRAPH.includes(module)),
  );
  // std.error uses std.inspect, which the checker declares.
  assert.ok(
    withStandardLibrary(program).uses.some(
      (use) => use.module === "std.inspect" && use.names.some(({ name }) => name === "Inspectable"),
    ),
  );
});

test("println is the std.console declaration, under its prelude name", () => {
  const program = parse('fn greet() -> void $ Console: println("hi")\n').program!;
  const println = withStandardLibrary(program).functions.find(
    (declaration) => declaration.name === "println",
  );
  assert.equal(println?.standard, true);
  const own = parse("fn println() -> void: pass\n").program!;
  assert.deepEqual(
    withStandardLibrary(own).functions.filter((declaration) => declaration.name === "println"),
    own.functions,
  );
});

test("only lib/std can declare an intrinsic", () => {
  // In user code the line is an ordinary decorator whose value calls an
  // unknown function (spec/lang/14-annotations.md#prefix-decorators).
  const analysis = analyze('@intrinsic("bytes_len")\nfn size(text: string) -> i32: 0\n');
  assert.deepEqual(
    analysis.diagnostics.map((diagnostic) => diagnostic.code),
    ["unknown-name"],
  );
});

test("a joined module declares every inherent method on a built-in type", () => {
  const program = parse("fn fallback(value: i32?) -> i32: value.unwrap_or(0)\n").program!;
  const joined = withStandardLibrary(program);
  const methods = joined.implementations.flatMap((implementation) =>
    implementation.standard && implementation.targetName === "T?"
      ? implementation.methods.map((method) => method.name)
      : [],
  );
  assert.ok(methods.includes("unwrap_or") && methods.includes("is_none"), methods.join(" "));
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
