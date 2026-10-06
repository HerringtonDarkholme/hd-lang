import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import type { HirExpression, HirProgram } from "../src/hir.ts";
import { classify } from "../src/highlight.ts";

// `usize` is a primitive of its own, not an alias of `u32`
// (spec/lang/04-type-system.md#the-usize-type).

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

test("usize is its own type throughout HIR", () => {
  const program = checked(`data Sizes:
    first: usize
    rest: List[usize]

fn same(value: usize) -> usize: value
fn nested(value: List[usize]) -> (usize, List[usize]): (value.len(), value)
fn cast(value: u32) -> usize: usize(value)
`);
  const sizes = program.data.find((declaration) => declaration.name === "Sizes")!;
  assert.deepEqual(
    sizes.fields.map((field) => field.type),
    ["usize", "List[usize]"],
  );
  const same = program.functions.find((declaration) => declaration.name === "same")!;
  assert.deepEqual([same.parameters[0]?.type, same.result], ["usize", "usize"]);
  const nested = program.functions.find((declaration) => declaration.name === "nested")!;
  assert.deepEqual(
    [nested.parameters[0]?.type, nested.result],
    ["List[usize]", "(usize,List[usize])"],
  );
  const cast = program.functions.find((declaration) => declaration.name === "cast")!;
  assert.equal(cast.result, "usize");
  assert.ok(
    expressions(program).some(
      (expression) =>
        expression.kind === "unary" &&
        expression.operator === "cast" &&
        expression.type === "usize",
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

test("usize is a generic parameter default", () => {
  const program = checked(`fn make[T = usize]() -> T:
    panic("not called")

fn use() -> usize: make()
`);
  const use = program.functions.find((declaration) => declaration.name === "use")!;
  const call = expressions(use.body).find((expression) => expression.kind === "call")!;
  assert.equal(call.type, "usize");
  assert.deepEqual(call.erasedTypeSubstitutions, [{ parameter: "T", type: "usize" }]);
});

test("usize stays usize inside generic trait arguments and associated bindings", () => {
  const program = checked(`use std.ops.{Index, Range}

trait Source[K]:
    type Item

fn direct[C < Index[usize]](value: C) -> void: pass
fn nested[C < Index[Range[usize]]](value: C) -> void: pass
fn associated[C < Source[usize, Item = usize]](value: C) -> void: pass
`);
  const bound = (name: string) =>
    program.functions.find((declaration) => declaration.name === name)!.genericBounds[0]!;
  assert.deepEqual(bound("direct").traitArguments, ["usize"]);
  assert.match(bound("nested").traitArguments[0]!, /Range\[usize\]$/);
  assert.deepEqual(bound("associated").traitArguments, ["usize"]);
  assert.deepEqual(bound("associated").associatedBindings, [{ name: "Item", type: "usize" }]);
});

test("List.len, Map.len, and a bare literal are usize in HIR", () => {
  const program =
    checked(`fn lengths(items: List[i32], entries: Map[string, i32]) -> (usize, usize, usize):
    count := 3
    (items.len(), entries.len(), count)
`);
  const declaration = program.functions.find((function_) => function_.name === "lengths")!;
  const lengths = expressions(declaration.body).filter(
    (expression) => expression.kind === "list-length" || expression.kind === "map-length",
  );
  assert.deepEqual(
    lengths.map((expression) => [expression.kind, expression.type]),
    [
      ["list-length", "usize"],
      ["map-length", "usize"],
    ],
  );
  assert.equal(declaration.locals.find((local) => local.name === "count")?.type, "usize");
});

test("the highlighter classifies usize as a type", () => {
  assert.equal(
    classify("let size: usize = items.len()").find(({ text }) => text === "usize")?.kind,
    "type",
  );
});

// A `u32` value and a size are two types; every place one meets the other
// is an error until a cast converts one (types.usize.convert).
const MIXED = `fn pick[T](a: T, b: T) -> T:
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

test("a u32 value and a size do not mix without a cast", () => {
  const uses: readonly (readonly [string, string])[] = [
    ["generic inference", "_ := pick(a, xs.len())"],
    ["a list literal", "_ := [a, xs.len()]"],
    [
      "a Map[u32, V] keyed by a size",
      'let keyed: Map[u32, string] = {7: "seven"}\n    _ := keyed[n]',
    ],
    ["if branches", "_ := if a > 3: a else: n"],
    ["a tuple", "let pair: (u32, u32) = (a, n)\n    _ := pair"],
    ["==", "_ := a == n"],
    ["arithmetic", "_ := a + n"],
    ["List[usize] passed as List[u32]", "let sizes: List[usize] = [n, n]\n    _ := total(sizes)"],
  ];
  for (const [name, use] of uses) {
    const { diagnostics } = analyze(`${MIXED}    ${use}\n`);
    assert.notDeepEqual(
      diagnostics.filter(({ severity }) => severity !== "warning"),
      [],
      name,
    );
  }
  const casts = [
    "_ := pick(usize(a), xs.len())",
    "_ := usize(a) + n",
    "_ := a == u32(n)",
    "_ := [usize(a), n]",
  ];
  for (const use of casts)
    assert.deepEqual(
      analyze(`${MIXED}    ${use}\n`).diagnostics.filter(({ severity }) => severity !== "warning"),
      [],
      use,
    );
});

test("a u32 operand beside a size offers a usize conversion", () => {
  const diagnostic = analyze(`fn run(xs: List[i32]) -> usize:
    let a: u32 = 1
    a + xs.len()
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(
    diagnostic?.message,
    "operator operands have types u32 and usize, which never convert implicitly; write usize(...)",
  );
  assert.deepEqual(
    diagnostic?.fix?.edits.map(({ replacement }) => replacement),
    ["usize(", ")"],
  );
});

test("a size passed for a u32 parameter offers a u32 conversion", () => {
  const diagnostic = analyze(`fn take_count(count: u32) -> u32: count

fn run(xs: List[i32]) -> u32:
    take_count(xs.len())
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(diagnostic?.message, "'usize' does not convert implicitly to 'u32'; write u32(...)");
  assert.equal(diagnostic?.fix?.message, "write 'u32(...)'");
});

test("a defaulted literal that meets a u32 suggests the annotation", () => {
  const diagnostic = analyze(`fn take_count(count: u32) -> u32: count

fn run() -> u32:
    total := 0
    take_count(total)
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.match(diagnostic?.notes?.[0] ?? "", /'total' is usize because its literal '0'/);
  assert.match(diagnostic?.notes?.[0] ?? "", /let total: u32 = 0/);
});

test("messages print the type: usize for a size, u32 for a u32", () => {
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

test("method lookup messages name the receiver's usize", () => {
  const message = (body: string): string | undefined =>
    analyze(`pub fn main() -> void:\n${body}`).diagnostics.find(
      ({ code }) => code === "unknown-method",
    )?.message;
  assert.match(
    message("    xs := [1]\n    xs.pussh(2)\n") ?? "",
    /^type 'List\[usize\]' has no supported method 'pussh'/,
  );
  assert.match(
    message('    let m: Map[string, usize] = {}\n    _ := m.remov("a")\n') ?? "",
    /^type 'Map\[string, usize\]' has no supported method 'remov'/,
  );
});

test("a written usize parameter is the expected type in a mismatch", () => {
  const diagnostic = analyze(`fn take_size(value: usize) -> void: pass

fn run() -> void:
    take_size("large")
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(diagnostic?.message, "expected usize, found string");
});

test("a field declared usize is read as usize", () => {
  const diagnostic = analyze(`data Sizes:
    count: usize

fn take_i32(value: i32) -> void: pass

fn run(sizes: Sizes) -> void:
    take_i32(sizes.count)
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(diagnostic?.message, "expected i32, found usize");
});

test("no-common-type lists a usize value", () => {
  const diagnostic = analyze(`fn run(size: usize) -> void:
    _ := [size, "many"]
`).diagnostics.find(({ code }) => code === "no-common-type");
  assert.equal(diagnostic?.message, "list elements have no common type: usize, string");
});

test("a generic-inference conflict lists a usize argument", () => {
  const diagnostic = analyze(`fn same[T](left: T, right: T) -> T: left

fn run(size: usize) -> void:
    _ := same(size, "many")
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(
    diagnostic?.message,
    "arguments of types 'usize' and 'string' both solve 'T' of 'same', and inference converts only 'mut X' to 'X'; convert one argument to the other's type",
  );
});

test("a length mismatch does not offer a literal edit for its receiver", () => {
  const diagnostic = analyze(`fn take_i32(value: i32) -> void: pass

fn run() -> void:
    xs := [1]
    take_i32(xs.len())
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(diagnostic?.message, "expected i32, found usize");
  assert.equal(diagnostic?.notes, undefined);
  assert.equal(diagnostic?.fix, undefined);
});

test("a literal bound through a tuple pattern is usize, with its hint", () => {
  const diagnostic = analyze(`fn work(left: i32, right: i32) -> void: pass

fn run() -> void:
    let (_a, b) = (1, 2)
    work(1, b)
`).diagnostics.find(({ code }) => code === "type-mismatch");
  assert.equal(diagnostic?.message, "expected i32, found usize");
  assert.match(diagnostic?.notes?.[0] ?? "", /'b' is usize because its literal '2'/);
});
