import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import type { HirExpression, HirFunction, HirStatement } from "../src/hir.ts";

function checked(source: string) {
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  assert.ok(result.hir);
  return result.hir;
}

function expressionOf(declaration: HirFunction): HirExpression {
  const statement = declaration.body[0];
  assert.equal(statement?.kind, "expression");
  return statement.expression;
}

function nestedExpressions(value: HirExpression | HirStatement): HirExpression[] {
  const expressions: HirExpression[] = [];
  const seen = new WeakSet<object>();
  const visit = (child: unknown): void => {
    if (Array.isArray(child)) return child.forEach(visit);
    if (!child || typeof child !== "object") return;
    if (seen.has(child)) return;
    seen.add(child);
    const record = child as Record<string, unknown>;
    if (typeof record.kind === "string" && typeof record.type === "string")
      expressions.push(child as HirExpression);
    for (const nested of Object.values(record)) visit(nested);
  };
  visit(value);
  return expressions;
}

test("built-in literal indices take the u32 usize context", () => {
  const hir = checked(`fn list_read(items: List[i32]) -> i32: items[0]
fn string_read(text: string) -> u8: text[0]
fn list_store(items: mut List[i32]) -> void:
    items[0] = 1
`);
  const expression = (name: string): HirExpression =>
    expressionOf(hir.functions.find((candidate) => candidate.name === name)!);

  const listRead = expression("list_read");
  const stringRead = expression("string_read");
  const listStore = expression("list_store");
  assert.equal(listRead.kind, "list-index");
  assert.equal(stringRead.kind, "string-index");
  assert.equal(listStore.kind, "list-set");
  assert.equal(listRead.index.type, "u32");
  assert.equal(stringRead.index.type, "u32");
  assert.equal(listStore.index.type, "u32");
});

test("slice literals default to u32 while a typed peer determines the bound width", () => {
  const hir = checked(`fn narrow(text: string) -> string: text[0..2]
fn positive(text: string) -> string: text[+0..+2]
fn compound(text: string) -> string: text[(0 + 1)..(2 + 3)]
fn byte(text: string, end: u8) -> string: text[0..end]
fn wide(text: string, end: u64) -> string: text[0..end]
`);
  const rangeType = (name: string): string => {
    const declaration = hir.functions.find((candidate) => candidate.name === name)!;
    const range = nestedExpressions(expressionOf(declaration)).find(
      (expression) => expression.kind === "data" && expression.type.includes("Range["),
    );
    assert.ok(range);
    return range.type;
  };
  assert.match(rangeType("narrow"), /Range\[u32\]$/);
  assert.match(rangeType("positive"), /Range\[u32\]$/);
  assert.match(rangeType("compound"), /Range\[u32\]$/);
  assert.match(rangeType("byte"), /Range\[u8\]$/);
  assert.match(rangeType("wide"), /Range\[u64\]$/);
});

test("a built-in IndexSet obligation supplies a generic literal key context", () => {
  const declarations = `use std.ops.IndexSet
fn put[K, V, C < mut IndexSet[K, V]](target: C, key: K, value: V) -> void:
    target[key] = value
`;
  const hir = checked(`${declarations}
fn run(numbers: mut List[i32]) -> void:
    put(numbers, 1, 9)
`);
  const call = expressionOf(hir.functions.find((candidate) => candidate.name === "run")!);
  assert.equal(call.kind, "call");
  assert.equal(call.arguments[1]?.type, "u32");

  const rejected = analyze(`${declarations}
fn invalid(numbers: mut List[i32]) -> void:
    put(numbers, -1, 9)
`);
  assert.equal(rejected.diagnostics[0]?.code, "unsigned-negation");
});

test("trait-bound inference is generic and forwards caller bounds", () => {
  const hir = checked(`trait Source[T]:
    fn take(self) -> T

data Label:
    text: string

impl Source[string] for Label:
    fn take(self) -> string:
        self.text

fn read[U, S < Source[U]](source: S) -> U:
    source.take()

fn read_unicode[Τ, S < Source[Τ]](source: S) -> Τ:
    source.take()

fn forward[X, S < Source[X]](source: S) -> X:
    value := read(source)
    value

fn label(source: Label) -> string:
    read(source)

fn unicode_label(source: Label) -> string:
    read_unicode(source)
`);
  const label = expressionOf(hir.functions.find((candidate) => candidate.name === "label")!);
  assert.equal(label.kind, "call");
  assert.equal(label.type, "string");
  const unicode = expressionOf(
    hir.functions.find((candidate) => candidate.name === "unicode_label")!,
  );
  assert.equal(unicode.kind, "call");
  assert.equal(unicode.type, "string");
});
