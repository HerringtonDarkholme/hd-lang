import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import type { HirExpression, HirProgram } from "../src/hir.ts";

function checkSource(source: string): [string, string][] {
  const result = analyze(source);
  return result.diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

function checked(source: string): HirProgram {
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  assert.ok(result.hir);
  return result.hir;
}

function expressions(value: unknown): HirExpression[] {
  const found: HirExpression[] = [];
  const visit = (child: unknown): void => {
    if (Array.isArray(child)) return child.forEach(visit);
    if (!child || typeof child !== "object") return;
    const node = child as Record<string, unknown>;
    if (typeof node.kind === "string" && typeof node.type === "string")
      found.push(node as unknown as HirExpression);
    for (const [key, nested] of Object.entries(node)) if (key !== "span") visit(nested);
  };
  visit(value);
  return found;
}

const SETUP = `data P:
    x: i32

data Box[T < Display]:
    value: T
`;

test("a written application checks its declaration's bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
fn top(b: Box[P]) -> void:
    println("t")

pub fn main() -> void $ Console:
    println("done")
`),
    [
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Display, required by the bound on 'T' of 'Box'",
      ],
    ],
  );
});

test("a nested written application checks its declaration's bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
fn nested(bs: List[Box[P]]) -> void:
    println("n")

fn count(ms: List[Map[P, i32]]) -> void:
    println("c")

pub fn main() -> void $ Console:
    println("done")
`),
    [
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Display, required by the bound on 'T' of 'Box'",
      ],
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Eq and Hash, required by the bound on 'K' of 'Map'",
      ],
    ],
  );
});

test("a written application accepts arguments that meet the bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
impl Display for P:
    fn to_string(self) -> string:
        "p"

fn forwarded[T < Display](b: Box[T]) -> void:
    println("f")

fn concrete(bs: List[Box[P]]) -> void:
    println("c")

pub fn main() -> void $ Console:
    println("done")
`),
    [],
  );
});

test("a bound implies its supertrait bounds in a data field", () => {
  assert.deepEqual(
    checkSource(`use std.cmp.{Ord, Eq}
data Box[T < Eq]:
    value: T

data Good[T < Ord]:
    b: Box[T]
`),
    [],
  );
});

test("a bound implies its supertrait bounds in a fn body", () => {
  assert.deepEqual(
    checkSource(`use std.cmp.{Ord, Eq}
data Box[T < Eq]:
    value: T

fn f[T < Ord](b: Box[T]) -> bool:
    b.value == b.value
`),
    [],
  );
});

test("a call proves a bound through a supertrait dictionary", () => {
  const program = checked(`trait Root:
    fn root(self) -> i32

trait Parent < Root:
    fn parent(self) -> i32

trait Child < Parent:
    fn child(self) -> i32

fn has_parent[T < Parent](value: T) -> i32:
    value.parent()

fn has_child[T < Child](value: T) -> i32:
    has_parent(value)
`);
  const child = program.functions.find((fn) => fn.name === "has_child")!;
  const dictionaries = expressions(child.body).flatMap((expression) =>
    expression.kind === "trait-bound-dictionary" ? [expression] : [],
  );
  assert.equal(dictionaries.length, 1);
  const parent = program.traits.find((trait) => trait.name === "Parent")!;
  const source = program.traits.find((trait) => trait.name === "Child")!;
  assert.deepEqual(
    dictionaries.map(({ traitIndex, supertrait }) => [traitIndex, supertrait]),
    [[parent.index, { sourceTraitIndex: source.index, path: [0] }]],
  );
});
