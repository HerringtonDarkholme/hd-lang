import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import type { HirExpression, HirProgram } from "../src/hir.ts";
import { classify } from "../src/highlight.ts";

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

test("the highlighter classifies usize as a type", () => {
  assert.equal(
    classify("let size: usize = items.len()").find(({ text }) => text === "usize")?.kind,
    "type",
  );
});

// A `u32` value and a size are one type wherever the checker compares types;
// the display spelling `usize` never splits them (r-types.alias.usize).
const MIXED = `trait Describe:
    fn describe(self) -> string

impl Describe for u32:
    fn describe(self) -> string:
        "u32 \${self}"

fn pick[T](a: T, b: T) -> T:
    a

fn total(values: List[u32]) -> u32:
    let sum: u32 = 0
    for value in values:
        sum = sum + value
    sum

fn mixed() -> void:
    xs := [1, 2]
    let a: u32 = 7
    n := xs.len()
`;

test("a u32 value and a size mix wherever one type is expected", () => {
  const uses: readonly (readonly [string, string])[] = [
    ["generic inference", "_ := pick(a, xs.len())"],
    ["a list literal", "_ := [a, xs.len()]"],
    ["a map literal", '_ := {a: "seven", n: "two"}'],
    ["a map value literal", '_ := {"a": a, "n": n}'],
    [
      "a Map[u32, V] keyed by a size",
      'let keyed: Map[u32, string] = {7: "seven"}\n    _ := keyed[n + 5]',
    ],
    [
      "a Map[usize, V] keyed by a u32",
      'let sized: mut Map[usize, string] = {}\n    sized[a] = "x"',
    ],
    ["if branches", "_ := if a > 3: a else: n"],
    ["match arms", "_ := match a:\n        7 => n\n        _ => a"],
    ["a tuple", "let pair: (u32, u32) = (a, n)\n    _ := pair"],
    ["== and <", "_ := a == n\n    _ := a < n"],
    ["arithmetic", "_ := a + n\n    _ := a * xs.len()\n    _ := n - a"],
    ["a trait implemented for u32", "_ := n.describe()"],
    ["List[usize] passed as List[u32]", "let sizes: List[usize] = [n, n]\n    _ := total(sizes)"],
    ["a list of both passed as List[u32]", "_ := total([n, a])"],
  ];
  for (const [name, use] of uses) {
    const { diagnostics } = analyze(`${MIXED}    ${use}\n`);
    assert.deepEqual(
      diagnostics.filter(({ severity }) => severity !== "warning"),
      [],
      name,
    );
  }
});

test("messages print a defaulted or written usize, not u32", () => {
  const mismatch = (body: string): string | undefined =>
    analyze(`fn take_i32(x: i32) -> i32: x\n\npub fn main() -> i32:\n${body}`).diagnostics.find(
      ({ code }) => code === "type-mismatch",
    )?.message;
  assert.equal(mismatch("    total := 0\n    take_i32(total)\n"), "expected i32, found usize");
  assert.equal(
    mismatch("    let total: usize = 0\n    take_i32(total)\n"),
    "expected i32, found usize",
  );
  assert.equal(
    mismatch("    let total: u32 = 0\n    take_i32(total)\n"),
    "expected i32, found u32",
  );
  assert.equal(mismatch("    xs := [1]\n    take_i32(xs.len())\n"), "expected i32, found usize");
  assert.equal(mismatch('    take_i32("abc".len())\n'), "expected i32, found usize");
  assert.equal(
    mismatch("    let a: u32 = 1\n    take_i32(usize(a))\n"),
    "expected i32, found usize",
  );
  assert.equal(mismatch("    let a: u32 = 1\n    take_i32(a + 1)\n"), "expected i32, found u32");
});
