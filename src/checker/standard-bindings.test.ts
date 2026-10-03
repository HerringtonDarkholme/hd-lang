import assert from "node:assert/strict";
import test from "node:test";

import { instantiate } from "../compiler.ts";
import type { Expression } from "../ast.ts";
import { parse } from "../parser/index.ts";
import { renameStandardBindings } from "./standard-bindings.ts";

function parsed(source: string) {
  const result = parse(source, { standardLibrary: true });
  assert.deepEqual(result.diagnostics, []);
  assert.ok(result.program);
  return result.program;
}

function named(expression: Expression): string {
  assert.equal(expression.kind, "name");
  return (expression as Extract<Expression, { kind: "name" }>).name;
}

test("std binding renames preserve variant, member, field, string, parameter and local names", () => {
  const program = parsed(`data Number:
    value: i32
enum Json:
    Number(value: Number)
trait Reader:
    fn read(self) -> i32
fn read() -> i32: 20
fn helper(Number: i32) -> Number:
    Number { value: Number }
fn use(reader: Reader) -> i32:
    let read = 22
    reader.read() + read
fn binding_user() -> i32:
    if (helper := 22) > 0:
        helper
    else:
        0
    helper
fn local_type() -> i32:
    data Number:
        next: Number?
    0
fn text() -> string: "Number read helper"
`);
  const renamed = renameStandardBindings(
    program,
    new Map(
      [
        "Number",
        "Json",
        "Reader",
        "read",
        "helper",
        "use",
        "binding_user",
        "local_type",
        "text",
      ].map((name) => [name, `__${name}`] as const),
    ),
  );

  assert.equal(renamed.data[0]?.name, "__Number");
  assert.equal(renamed.enums[0]?.name, "__Json");
  assert.equal(renamed.enums[0]?.variants[0]?.name, "Number");
  assert.equal(renamed.enums[0]?.variants[0]?.fields[0]?.name, "value");
  assert.equal(renamed.enums[0]?.variants[0]?.fields[0]?.type.name, "__Number");
  assert.equal(renamed.traits[0]?.methods[0]?.name, "read");

  const helper = renamed.functions.find((item) => item.name === "__helper")!;
  assert.equal(helper.parameters[0]?.name, "Number");
  assert.equal(helper.result.name, "__Number");
  const construction = helper.body[0];
  assert.equal(construction?.kind, "expression");
  assert.equal(
    construction?.kind === "expression" && construction.expression.kind === "data"
      ? construction.expression.name
      : undefined,
    "__Number",
  );
  assert.equal(
    construction?.kind === "expression" && construction.expression.kind === "data"
      ? named(construction.expression.fields[0]!.value)
      : undefined,
    "Number",
  );

  const use = renamed.functions.find((item) => item.name === "__use")!;
  const result = use.body[1];
  assert.equal(result?.kind, "expression");
  const binary = result?.kind === "expression" ? result.expression : undefined;
  assert.equal(binary?.kind, "binary");
  assert.equal(binary?.kind === "binary" ? named(binary.right) : undefined, "read");
  const memberCall = binary?.kind === "binary" ? binary.left : undefined;
  assert.equal(
    memberCall?.kind === "call" && memberCall.callee.kind === "member"
      ? memberCall.callee.name
      : undefined,
    "read",
  );

  const bindingUser = renamed.functions.find((item) => item.name === "__binding_user")!;
  const conditional =
    bindingUser.body[0]?.kind === "expression" ? bindingUser.body[0].expression : undefined;
  assert.equal(conditional?.kind, "if");
  assert.equal(
    conditional?.kind === "if" && conditional.thenBody[0]?.kind === "expression"
      ? named(conditional.thenBody[0].expression)
      : undefined,
    "helper",
  );
  assert.equal(
    bindingUser.body[1]?.kind === "expression" ? named(bindingUser.body[1].expression) : undefined,
    "helper",
  );

  const localType = renamed.functions.find((item) => item.name === "__local_type")!;
  const localDeclaration = localType.body[0];
  assert.equal(localDeclaration?.kind, "local-declaration");
  assert.equal(
    localDeclaration?.kind === "local-declaration" && localDeclaration.declaration.kind === "data"
      ? localDeclaration.declaration.fields[0]?.type.name
      : undefined,
    "Number?",
  );
  const text = renamed.functions.find((item) => item.name === "__text")!.body[0];
  assert.equal(
    text?.kind === "expression" && text.expression.kind === "string"
      ? text.expression.value
      : undefined,
    "Number read helper",
  );
});

test("free calls with a trait method's spelling bind to the top-level declaration", () => {
  const program = parsed(`trait Reader:
    fn read(self) -> i32
fn read() -> i32: 42
fn use(reader: Reader) -> i32:
    reader.read() + read()
`);
  const renamed = renameStandardBindings(
    program,
    new Map([
      ["Reader", "__Reader"],
      ["read", "__read"],
      ["use", "__use"],
    ]),
  );
  const expression = renamed.functions.find((item) => item.name === "__use")!.body[0];
  assert.equal(expression?.kind, "expression");
  const binary = expression?.kind === "expression" ? expression.expression : undefined;
  assert.equal(binary?.kind, "binary");
  assert.equal(
    binary?.kind === "binary" && binary.right.kind === "call"
      ? named(binary.right.callee)
      : undefined,
    "__read",
  );
  assert.equal(
    binary?.kind === "binary" && binary.left.kind === "call" && binary.left.callee.kind === "member"
      ? binary.left.callee.name
      : undefined,
    "read",
  );
});

test("std JSON variants and Duration display survive declaration renaming", async () => {
  const source = `use std.json.{Json, Number}
use std.time.Duration
fn main() -> i32:
    value := Json.Number(Number::from_i64(42))
    if value == Json.Number(Number::from_i64(42)) && "\${Duration::milliseconds(500)}" == "500ms":
        42
    else:
        0
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
