import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import type { HirExpression, HirProgram } from "../src/hir.ts";

const U64_MAX = "18446744073709551615";

function checked(source: string): HirProgram {
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  return result.hir!;
}

function resultExpression(program: HirProgram, name: string): HirExpression {
  const body = program.functions.find((candidate) => candidate.name === name)!.body;
  const statement = body[0]!;
  assert.equal(statement.kind, "expression");
  return statement.expression;
}

function integerLeaves(value: unknown): Array<Extract<HirExpression, { kind: "integer" }>> {
  const leaves: Array<Extract<HirExpression, { kind: "integer" }>> = [];
  const visit = (child: unknown): void => {
    if (Array.isArray(child)) {
      child.forEach(visit);
      return;
    }
    if (child === null || typeof child !== "object") return;
    if ((child as { kind?: unknown }).kind === "integer") {
      leaves.push(child as Extract<HirExpression, { kind: "integer" }>);
      return;
    }
    for (const nested of Object.values(child)) visit(nested);
  };
  visit(value);
  return leaves;
}

test("a typed comparison operand contextualizes a nested wide integer expression", () => {
  const program = checked(`fn within(magnitude: u64, digit: u64) -> bool:
    magnitude > (${U64_MAX} - digit) / 10
`);
  const comparison = resultExpression(program, "within");
  assert.equal(comparison.kind, "value-ordering");
  assert.equal(comparison.valueType, "u64");
  assert.equal(comparison.right.type, "u64");
  assert.deepEqual(
    integerLeaves(comparison.right).map(({ type, wide }) => [type, wide]),
    [
      ["u64", U64_MAX],
      ["u64", "10"],
    ],
  );
});

test("return, call, and data-field expectations reach nested integer expressions", () => {
  const program = checked(`data Box:
    value: u64

fn take(value: u64) -> u64: value

fn returned(digit: u64) -> u64:
    (${U64_MAX} - digit) / 10

fn called(digit: u64) -> u64:
    take((${U64_MAX} - digit) / 10)

fn field(digit: u64) -> Box:
    Box { value: (${U64_MAX} - digit) / 10 }
`);
  for (const name of ["returned", "called", "field"]) {
    const leaves = integerLeaves(resultExpression(program, name));
    assert.equal(leaves.find(({ wide }) => wide === U64_MAX)?.type, "u64");
    assert.ok(leaves.every(({ type }) => type === "u64"));
  }
});

test("unary and bitwise trees retain a wide expected integer type", () => {
  const program = checked(`fn positive() -> u64:
    +${U64_MAX}

fn minimum(value: i64) -> i64:
    (-9223372036854775808 - value) / 10

fn masked() -> u64:
    (${U64_MAX} & 7) | 2
`);
  assert.equal(integerLeaves(resultExpression(program, "positive"))[0]!.type, "u64");
  assert.deepEqual(
    integerLeaves(resultExpression(program, "minimum")).map(({ type, wide }) => [type, wide]),
    [
      ["i64", "-9223372036854775808"],
      ["i64", "10"],
    ],
  );
  assert.ok(integerLeaves(resultExpression(program, "masked")).every(({ type }) => type === "u64"));
});

test("shift and power context only their result-typed base", () => {
  const program = checked(`fn shifted() -> u64:
    ${U64_MAX} << 1

fn powered() -> u64:
    ${U64_MAX} ** 1
`);
  const shifted = resultExpression(program, "shifted");
  assert.equal(shifted.kind, "binary");
  assert.deepEqual([shifted.left.type, shifted.right.type, shifted.type], ["u64", "u32", "u64"]);
  const powered = resultExpression(program, "powered");
  assert.equal(powered.kind, "binary");
  assert.deepEqual([powered.left.type, powered.right.type, powered.type], ["u64", "i32", "u64"]);
});

test("an immediate left literal still adopts a primitive right operand type", () => {
  const program = checked(`fn ordered(value: u64) -> bool:
    1 < value
`);
  const comparison = resultExpression(program, "ordered");
  assert.equal(comparison.kind, "value-ordering");
  assert.deepEqual(
    [comparison.left.type, comparison.right.type, comparison.valueType],
    ["u64", "u64", "u64"],
  );
});

test("a literal argument takes the width another argument solves for its type parameter", () => {
  const program = checked(`fn biggest[T < Ord](left: T, right: T) -> T:
    if left > right: left else: right

fn apply[B](init: B, step: fn(B) -> B) -> B:
    step(init)

fn widest(sizes: List[usize]) -> usize:
    biggest(0, sizes.len())

fn total(sizes: List[usize]) -> usize:
    sizes.iter().fold(0, fn(acc: usize, n: usize) -> usize: acc + n)

fn stepped() -> i64:
    apply(-1, fn(value: i64) -> i64: value + 1)

fn counted(sizes: List[usize]) -> i32:
    sizes.iter().fold(0, fn(acc, n): acc + 1)
`);
  const literalTypes = (name: string): string[] =>
    integerLeaves(program.functions.find((candidate) => candidate.name === name)!.body).map(
      ({ type }) => type,
    );
  assert.deepEqual(literalTypes("widest"), ["u32"]);
  assert.deepEqual(literalTypes("total"), ["u32"]);
  assert.deepEqual(literalTypes("stepped"), ["i64"]);
  // A closure with an unannotated parameter reads the literal's own width.
  assert.deepEqual(literalTypes("counted"), ["i32"]);
});
