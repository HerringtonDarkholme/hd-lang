import assert from "node:assert/strict";
import test from "node:test";

import { instantiate } from "../compiler.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";

test("generated types do not capture copied member, field, or variant names", () => {
  const source = new Source_();
  const type = source.type("Box");
  source.add(`fn get(value: ${type}) -> i32:`);
  source.add("    value.HDTYPE0X");
  source.add(`fn make(value: i32) -> ${type}:`);
  source.add("    Box { HDTYPE0X: value }");
  source.add("fn variant() -> Choice:");
  source.add("    Choice.HDTYPE0X");
  const program = source.program(ZERO_SPAN);
  assert.equal(program.functions[0]!.parameters[0]!.type.name, "Box");
  assert.equal(program.functions[1]!.result.name, "Box");
  const body = program.functions.map((fn) => fn.body[0]);
  assert.deepEqual(body[0], {
    kind: "expression",
    expression: {
      kind: "member",
      receiver: { kind: "name", name: "value", span: ZERO_SPAN },
      name: "HDTYPE0X",
      span: ZERO_SPAN,
    },
    span: ZERO_SPAN,
  });
  assert.ok(body[1]?.kind === "expression" && body[1].expression.kind === "data");
  assert.equal(body[1].expression.fields[0]!.name, "HDTYPE0X");
  assert.ok(body[2]?.kind === "expression" && body[2].expression.kind === "member");
  assert.equal(body[2].expression.name, "HDTYPE0X");
});

test("expression handles are distinct from every legal user spelling", () => {
  const source = new Source_();
  const expression = { kind: "integer", value: 7n, span: ZERO_SPAN } as const;
  const handle = source.expression(expression);
  source.add("fn get(hdexpr0: i32, hd__generated_0: i32, hdexpr999999: i32) -> i32:");
  source.add(`    (hdexpr0, hd__generated_0, hdexpr999999, ${handle})._0`);
  const program = source.program(ZERO_SPAN);
  const statement = program.functions[0]!.body[0]!;
  assert.ok(statement.kind === "expression" && statement.expression.kind === "member");
  assert.ok(statement.expression.receiver.kind === "tuple");
  assert.deepEqual(
    statement.expression.receiver.elements.slice(0, 3).map((node) => {
      assert.ok(node.kind === "name");
      return node.name;
    }),
    ["hdexpr0", "hd__generated_0", "hdexpr999999"],
  );
  assert.equal(statement.expression.receiver.elements[3], expression);
});

test("type handles replace only issued tokens, once, in nested type positions", () => {
  const source = new Source_();
  const first = source.type("HDTYPE1X");
  const second = source.type("i32");
  source.add(`fn get(value: Map[${second}, List[${first}]]) -> (${first}, ${second}):`);
  source.add("    value.HDTYPE999999X");
  const program = source.program(ZERO_SPAN);
  assert.equal(program.functions[0]!.parameters[0]!.type.name, "Map[i32,List[HDTYPE1X]]");
  assert.equal(program.functions[0]!.result.name, "(HDTYPE1X,i32)");
  const statement = program.functions[0]!.body[0]!;
  assert.ok(statement.kind === "expression" && statement.expression.kind === "member");
  assert.equal(statement.expression.name, "HDTYPE999999X");
});

test("placeholder allocation excludes names introduced by the rename callback", () => {
  const source = new Source_();
  const expression = { kind: "integer", value: 7n, span: ZERO_SPAN } as const;
  const handle = source.expression(expression);
  source.add("fn original() -> i32:");
  source.add("    1");
  source.add("fn get() -> i32:");
  source.add(`    original() + ${handle}`);
  const program = source.program(ZERO_SPAN, (text) =>
    text.replaceAll("original", "hd__generated_0"),
  );
  assert.equal(program.functions[0]!.name, "hd__generated_0");
  const statement = program.functions[1]!.body[0]!;
  assert.ok(statement.kind === "expression" && statement.expression.kind === "binary");
  assert.ok(statement.expression.left.kind === "call");
  assert.ok(statement.expression.left.callee.kind === "name");
  assert.equal(statement.expression.left.callee.name, "hd__generated_0");
  assert.equal(statement.expression.right, expression);
});

test("renaming, source provenance, literal contents, and inserted AST are preserved", () => {
  const source = new Source_();
  const inserted = {
    kind: "name",
    name: "hdexpr0",
    span: { start: { line: 9, column: 2, offset: 20 }, end: { line: 9, column: 9, offset: 27 } },
  } as const;
  const type = source.type("HDTYPE0X");
  const handle = source.expression(inserted);
  source.add(`fn original(value: ${type}) -> string:`);
  source.add('    "HDTYPE0X hdexpr0 hd__generated_0"', inserted.span);
  source.add("fn preserved() -> i32:");
  source.add(`    ${handle}`);
  const program = source.program(ZERO_SPAN, (text) => text.replace("original", "renamed"));
  assert.equal(program.functions[0]!.name, "renamed");
  assert.equal(program.functions[0]!.parameters[0]!.type.name, "HDTYPE0X");
  const literal = program.functions[0]!.body[0]!;
  assert.ok(literal.kind === "expression" && literal.expression.kind === "string");
  assert.equal(literal.expression.value, "HDTYPE0X hdexpr0 hd__generated_0");
  assert.deepEqual(literal.expression.span, inserted.span);
  const statement = program.functions[1]!.body[0]!;
  assert.ok(statement.kind === "expression");
  assert.equal(statement.expression, inserted);
});

test("typed derivation preserves placeholder-shaped data members and enum variants", async () => {
  const { instance } = await instantiate(`
@derive(Eq)
data Box:
    HDTYPE0X: i32
    HDTYPE999999X: i32

@derive(Eq)
enum Choice:
    HDTYPE0X(i32)
    HDTYPE999999X

fn main() -> i32:
    left := Box { HDTYPE0X: 1, HDTYPE999999X: 2 }
    right := Box { HDTYPE0X: 1, HDTYPE999999X: 2 }
    if left == right && Choice.HDTYPE0X(3) == Choice.HDTYPE0X(3) && Choice.HDTYPE999999X == Choice.HDTYPE999999X: 1 else: 0
`);
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});
