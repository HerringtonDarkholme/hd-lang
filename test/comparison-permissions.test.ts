import assert from "node:assert/strict";
import test from "node:test";

import type { HirExpression } from "../src/hir.ts";
import { analyze } from "../src/compiler.ts";

const declarations = `data Point:
    x: i32

impl Eq for Point:
    fn eq(self, other: Self) -> bool:
        self.x == other.x

impl PartialOrd for Point:
    fn partial_cmp(self, other: Self) -> Ordering?:
        if self.x < other.x:
            Ordering.Less
        else if self.x > other.x:
            Ordering.Greater
        else:
            Ordering.Equal

fn point() -> Point:
    Point { x: 1 }
`;

function resultExpression(source: string, name: string): HirExpression {
  const result = analyze(`${declarations}\n${source}`);
  assert.deepEqual(result.diagnostics, []);
  const body = result.hir!.functions.find((candidate) => candidate.name === name)!.body;
  const statement = body[0]!;
  assert.equal(statement.kind, "expression");
  return statement.expression;
}

function equalityOperandTypes(expression: HirExpression): readonly [string, string, string] {
  const equality = expression.kind === "unary" ? expression.operand : expression;
  assert.equal(equality.kind, "value-equality");
  return [equality.left.type, equality.right.type, equality.valueType];
}

test("equality and inequality normalize an outer permission in either operand order", () => {
  const source = `fn equal() -> bool:
    point() == Point { x: 1 }

fn unequal() -> bool:
    Point { x: 1 } != point()
`;

  assert.deepEqual(equalityOperandTypes(resultExpression(source, "equal")), [
    "Point",
    "Point",
    "Point",
  ]);
  assert.deepEqual(equalityOperandTypes(resultExpression(source, "unequal")), [
    "Point",
    "Point",
    "Point",
  ]);
});

test("ordering normalizes an outer permission in either operand order", () => {
  const source = `fn less() -> bool:
    point() < Point { x: 2 }

fn at_least() -> bool:
    Point { x: 2 } >= point()
`;

  for (const name of ["less", "at_least"]) {
    const expression = resultExpression(source, name);
    assert.equal(expression.kind, "value-ordering");
    assert.deepEqual(
      [expression.left.type, expression.right.type, expression.valueType],
      ["Point", "Point", "Point"],
    );
  }
});

test("named readonly and mutable outer views compare in either order", () => {
  const result = analyze(`${declarations}
fn compare(readonly: Point, mutable: mut Point) -> bool:
    readonly == mutable && mutable == readonly
`);

  assert.deepEqual(result.diagnostics, []);
});

test("outer list permission normalizes without removing its element permission", () => {
  const result = analyze(`${declarations}
fn compare(readonly: List[mut Point], mutable: mut List[mut Point]) -> bool:
    readonly == mutable && mutable != readonly
`);

  assert.deepEqual(result.diagnostics, []);
});

test("a nested permission difference remains a comparison type mismatch", () => {
  const result = analyze(`${declarations}
fn compare(readonly: List[Point], mutable_elements: List[mut Point]) -> bool:
    readonly == mutable_elements
`);

  assert.deepEqual(
    result.diagnostics.map(({ code, message }) => ({ code, message })),
    [
      {
        code: "type-mismatch",
        message: "operator operands have types List[Point] and List[mut Point]",
      },
    ],
  );
});
