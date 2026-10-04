import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import type { HirExpression, HirProgram } from "../src/hir.ts";
import { classify } from "../src/highlight.ts";
import { RuntimePanicError } from "../src/runtime-panic.ts";
import { PRELUDE_ORIGINS } from "../src/checker/prelude-names.ts";

function checked(source: string): HirProgram {
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.ok(analysis.hir);
  return analysis.hir;
}

function expressions(value: unknown): HirExpression[] {
  const found: HirExpression[] = [];
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) return value.forEach(visit);
    if (!value || typeof value !== "object") return;
    const node = value as Record<string, unknown>;
    if (typeof node.kind === "string" && typeof node.type === "string")
      found.push(node as unknown as HirExpression);
    for (const [key, child] of Object.entries(node)) if (key !== "span") visit(child);
  };
  visit(value);
  return found;
}

test("the std.core usize alias is canonical u32 throughout HIR", () => {
  const program = checked(`data Sizes:
    first: usize
    rest: List[usize]

fn same(value: usize) -> u32: value
fn nested(value: List[usize]) -> (usize, List[usize]): (value.len(), value)
fn cast(value: i32) -> usize: usize(value)
`);
  const sizes = program.data.find((declaration) => declaration.name === "Sizes")!;
  assert.deepEqual(
    sizes.fields.map((field) => field.type),
    ["u32", "List[u32]"],
  );
  const same = program.functions.find((declaration) => declaration.name === "same")!;
  assert.deepEqual([same.parameters[0]?.type, same.result], ["u32", "u32"]);
  const nested = program.functions.find((declaration) => declaration.name === "nested")!;
  assert.deepEqual([nested.parameters[0]?.type, nested.result], ["List[u32]", "(u32,List[u32])"]);
  const cast = program.functions.find((declaration) => declaration.name === "cast")!;
  assert.equal(cast.result, "u32");
  assert.ok(
    expressions(program).some(
      (expression) =>
        expression.kind === "unary" && expression.operator === "cast" && expression.type === "u32",
    ),
  );

  const withoutSpans = JSON.stringify(program, (key, value) =>
    key === "span" ? undefined : value,
  );
  assert.doesNotMatch(withoutSpans, /\busize\b/);
});

test("a renamed explicit std.core usize import keeps alias identity", () => {
  const program = checked(`use std.core.usize as Size

fn same(value: Size) -> u32: value
fn cast(value: i32) -> Size: Size(value)
`);
  const same = program.functions.find((declaration) => declaration.name === "same")!;
  const cast = program.functions.find((declaration) => declaration.name === "cast")!;
  assert.deepEqual([same.parameters[0]?.type, same.result, cast.result], ["u32", "u32", "u32"]);
  assert.ok(
    expressions(program).some(
      (expression) =>
        expression.kind === "unary" && expression.operator === "cast" && expression.type === "u32",
    ),
  );
});

test("usize expands in a transparent alias target", () => {
  const program = checked(`type Size = usize

fn same(value: Size) -> u32: value
`);
  const same = program.functions.find((declaration) => declaration.name === "same")!;
  assert.deepEqual([same.parameters[0]?.type, same.result], ["u32", "u32"]);
});

test("alias expansion preserves an inherent implementation's written target", () => {
  const diagnostics = analyze(`data User:
    value: i32

type Person = User

impl Person:
    fn value(self) -> i32: self.value
`).diagnostics;
  assert.deepEqual(
    diagnostics.map((diagnostic) => diagnostic.code),
    ["invalid-impl-target"],
  );
});

test("usize still expands within trait and implementation heads", () => {
  const program = checked(`trait Marker[T]:
    fn mark(self) -> T

data Wrapper[T]:
    value: T

impl Marker[usize] for Wrapper[usize]:
    fn mark(self) -> usize: self.value
`);
  const implementation = program.implementations.find(
    (candidate) => candidate.traitName === "Marker",
  )!;
  assert.deepEqual(implementation.traitArguments, ["u32"]);
  assert.equal(implementation.targetType, "Wrapper[u32]");
});

test("usize expands in a newtype base", () => {
  const program = checked(`type Size(usize)
`);
  const size = program.data.find((declaration) => declaration.name === "Size")!;
  assert.equal(size.newtype, true);
  assert.equal(size.fields[0]?.type, "u32");
});

test("usize expands in a generic parameter default", () => {
  const program = checked(`fn make[T = usize]() -> T:
    panic("not called")

fn use() -> u32: make()
`);
  const use = program.functions.find((declaration) => declaration.name === "use")!;
  const call = expressions(use.body).find((expression) => expression.kind === "call")!;
  assert.equal(call.type, "u32");
  assert.deepEqual(call.erasedTypeSubstitutions, [{ parameter: "T", type: "u32" }]);
});

test("usize expands inside generic trait arguments and associated bindings", () => {
  const program = checked(`use std.ops.{Index, Range}

trait Source[K]:
    type Item

fn direct[C < Index[usize]](value: C) -> void: pass
fn nested[C < Index[Range[usize]]](value: C) -> void: pass
fn associated[C < Source[usize, Item = usize]](value: C) -> void: pass
`);
  const bound = (name: string) =>
    program.functions.find((declaration) => declaration.name === name)!.genericBounds[0]!;
  assert.deepEqual(bound("direct").traitArguments, ["u32"]);
  assert.match(bound("nested").traitArguments[0]!, /Range\[u32\]$/);
  assert.deepEqual(bound("associated").traitArguments, ["u32"]);
  assert.deepEqual(bound("associated").associatedBindings, [{ name: "Item", type: "u32" }]);
});

test("usize remains a protected prelude name", () => {
  assert.equal(PRELUDE_ORIGINS.get("usize"), "std.core");
  const codes = (source: string): readonly string[] =>
    analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
  assert.deepEqual(codes("type usize = u32\n"), ["prelude-name-shadow"]);
  assert.deepEqual(codes("fn identity[usize](value: usize) -> usize: value\n"), [
    "prelude-name-shadow",
  ]);
  assert.deepEqual(codes("fn local() -> void:\n    usize := 1\n"), ["prelude-name-shadow"]);
});

test("List.len and Map.len have canonical u32 HIR types", () => {
  const program =
    checked(`fn lengths(items: List[i32], entries: Map[string, i32]) -> (usize, usize):
    (items.len(), entries.len())
`);
  const declaration = program.functions.find((function_) => function_.name === "lengths")!;
  const lengths = expressions(declaration.body).filter(
    (expression) => expression.kind === "list-length" || expression.kind === "map-length",
  );
  assert.deepEqual(
    lengths.map((expression) => [expression.kind, expression.type]),
    [
      ["list-length", "u32"],
      ["map-length", "u32"],
    ],
  );
});

test("subtracting one from an empty list length has u32 overflow semantics", async () => {
  const { instance } = await instantiate(`fn underflow() -> usize:
    let items: List[i32] = []
    items.len() - 1

pub fn main() -> void:
    _ := underflow()
`);
  assert.throws(
    () => (instance.exports.main as CallableFunction)(),
    (error: unknown) => error instanceof RuntimePanicError && error.code === "integer-overflow",
  );
});

test("the highlighter classifies usize as a type", () => {
  assert.equal(
    classify("let size: usize = items.len()").find(({ text }) => text === "usize")?.kind,
    "type",
  );
});
