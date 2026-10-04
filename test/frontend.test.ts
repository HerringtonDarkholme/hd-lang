import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { lex } from "../src/lexer.ts";
import { parse } from "../src/parser/index.ts";
import { conformanceBody, fixtureBody } from "./fixture.ts";

const CORE_PROGRAM = fixtureBody("frontend/00-core-program");

test("lexer emits layout tokens and source positions", () => {
  const result = lex(CORE_PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.tokens.filter((token) => token.kind === "indent").length, 4);
  assert.equal(result.tokens.filter((token) => token.kind === "dedent").length, 4);
  const choose = result.tokens.find((token) => token.text === "choose");
  assert.deepEqual(choose?.span.start, { offset: 3, line: 1, column: 4 });
});

test("lexer keeps literal suffixes on decimal and float numbers only", () => {
  const result = lex("a := 250ms\nb := 1e3ms\nc := 5em\nd := 0xffB\ne := 1e3\n");
  assert.deepEqual(result.diagnostics, []);
  const numbers = result.tokens.filter(
    (token) => token.kind === "integer" || token.kind === "float",
  );
  assert.deepEqual(
    numbers.map((token) => [token.text, token.suffix?.name]),
    [
      ["250ms", "ms"],
      ["1e3ms", "ms"],
      ["5em", "em"],
      ["0xffB", undefined],
      ["1e3", undefined],
    ],
  );
  assert.deepEqual(
    lex("f := 5_ms\n").diagnostics.map((diagnostic) => diagnostic.code),
    ["invalid-token"],
  );
  assert.deepEqual(
    lex("g := 5else\n").diagnostics.map((diagnostic) => diagnostic.code),
    ["syntax-error"],
  );
  assert.deepEqual(
    lex("h := 0xff'B\n").diagnostics.map((diagnostic) => diagnostic.code),
    ["unterminated-string"],
  );
});

test("parser desugars a suffixed literal to a call of its suffix function, under a minus", () => {
  const program = parse("fn f() -> i32:\n    -5px\n").program!;
  const expression = program.functions[0]!.body.at(-1);
  assert.ok(expression?.kind === "expression");
  const negation = expression.expression;
  assert.ok(negation.kind === "unary" && negation.operator === "-");
  const call = negation.operand;
  assert.ok(call.kind === "call" && call.literalSuffix === "px");
  assert.equal(call.callee.kind === "name" && call.callee.name, "px");
  assert.equal(call.arguments[0]?.kind, "integer");
});

test("lexer keeps a prefixed string raw and rejects a reserved word glued to a quote", () => {
  const [prefixed] = lex('sql"a\\n $x $5"\n').tokens;
  assert.equal(prefixed?.prefix?.name, "sql");
  const value = prefixed?.value;
  assert.ok(typeof value === "object" && value.kind === "interpolated-string");
  assert.deepEqual(
    value.segments.map((segment) => (segment.kind === "text" ? segment.value : segment.source)),
    ["a\\n ", "x", " $5"],
  );
  const glued = lex('return"x"\n');
  assert.deepEqual(
    glued.tokens.slice(0, 2).map((token) => token.kind),
    ["keyword", "string"],
  );
  assert.deepEqual(
    glued.diagnostics.map((diagnostic) => diagnostic.code),
    ["syntax-error"],
  );
});

test("parser desugars a prefixed string to a call with a template of pieces and values", () => {
  const program = parse('fn f() -> i32:\n    q"$a$b c"\n').program!;
  const expression = program.functions[0]!.body.at(-1);
  assert.ok(expression?.kind === "expression");
  const call = expression.expression;
  assert.ok(call.kind === "call" && call.stringPrefix === "q");
  const template = call.arguments[0];
  assert.ok(template?.kind === "data");
  const [pieces, values] = template.fields.map((field) => field.value);
  assert.ok(pieces?.kind === "list" && values?.kind === "list");
  assert.deepEqual(
    pieces.elements.map((piece) => piece.kind === "string" && piece.value),
    ["", "", " c"],
  );
  assert.equal(values.elements.length, 2);
});

test("parser builds functions, bindings, calls, and value-producing if", () => {
  const result = parse(CORE_PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions.length, 2);
  assert.equal(result.program?.functions[0]?.body[0]?.kind, "expression");
  assert.equal(result.program?.functions[1]?.body[0]?.kind, "binding");
});

test("core AST shape matches its checked-in snapshot", () => {
  const result = parse(CORE_PROGRAM);
  const actual =
    JSON.stringify(
      result.program,
      (key, value) => {
        if (key === "span") return undefined;
        return typeof value === "bigint" ? `${value}n` : value;
      },
      2,
    ) + "\n";
  assert.equal(actual, readFileSync(resolve("test/snapshots/core.ast.json"), "utf8"));
});

test("parser builds value-producing while else", () => {
  const result = parse(conformanceBody("parse/valid/while-else"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  assert.equal(statement?.kind, "expression");
  if (statement?.kind === "expression" && statement.expression.kind === "while") {
    assert.equal(statement.expression.elseBody.length, 1);
  }
});

test("parser builds for loops with tuple bindings and else suites", () => {
  const result = parse(conformanceBody("parse/valid/for-tuple-binding-else"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "for") {
    assert.deepEqual(
      statement.expression.bindings.map((binding) => binding.name),
      ["key", "value"],
    );
    assert.equal(statement.expression.elseBody.length, 1);
  } else {
    assert.fail("expected a for expression");
  }
});

test("parser lowers named local functions to typed closure bindings", () => {
  const result = parse(conformanceBody("parse/valid/named-local-function"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "binding") {
    assert.equal(statement.name, "add");
    // Without a requirement clause the row is inferred, so no annotation fixes it.
    assert.equal(statement.annotation, undefined);
    assert.equal(statement.value.kind, "closure");
    if (statement.value.kind === "closure") {
      assert.equal(statement.value.result?.name, "i32");
      assert.equal(statement.value.requirements, undefined);
    }
  } else {
    assert.fail("expected a local closure binding");
  }
});

test("parser retains mutable permission types and field assignments", () => {
  const result = parse(conformanceBody("parse/valid/mutable-parameter-field-assignment"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[0]?.type.name, "mut:User");
  assert.equal(result.program?.functions[0]?.result.name, "mut:User");
  assert.equal(result.program?.functions[0]?.body[0]?.kind, "field-assignment");
  assert.equal(
    parse(conformanceBody("parse/valid/mutable-list-parameter-index-assignment")).program
      ?.functions[0]?.body[0]?.kind,
    "index-assignment",
  );
});

test("parser represents mut self as a mutable Self receiver", () => {
  const result = parse(conformanceBody("parse/valid/mut-self-receiver"));
  assert.deepEqual(result.diagnostics, []);
  const receiver = result.program?.traits[0]?.methods[0]?.parameters[0];
  assert.equal(receiver?.name, "self");
  assert.equal(receiver?.type.name, "mut:Self");

  assert.equal(
    parse(conformanceBody("parse/invalid/mut-non-self-parameter")).diagnostics[0]?.code,
    "syntax-error",
  );
});

test("a pattern before := is missing-let whose fix-it writes the let statement", () => {
  // 02-grammar.md#r-grammar.stmt.short-binding.let-only
  for (const [pattern, written] of [
    ["(a, b) := pair", "let (a, b) = pair"],
    ["Point { x, y } := p", "let Point { x, y } = p"],
    [".Some(v) := found", "let .Some(v) = found"],
  ] as const) {
    const source = `fn f() -> void:\n    ${pattern}\n`;
    const [diagnostic] = parse(source).diagnostics;
    assert.equal(diagnostic?.code, "missing-let");
    const edits = [...(diagnostic?.fix?.edits ?? [])].sort(
      (left, right) => right.span.start.offset - left.span.start.offset,
    );
    const fixed = edits.reduce(
      (text, edit) =>
        text.slice(0, edit.span.start.offset) + edit.replacement + text.slice(edit.span.end.offset),
      source,
    );
    assert.equal(fixed, `fn f() -> void:\n    ${written}\n`);
  }
});

test("missing-let for a typed binding names the type annotation, not mutability", () => {
  // 02-grammar.md#r-grammar.stmt.typed-binding.let
  const [diagnostic] = parse("fn f() -> void:\n    x: i32 = 1\n").diagnostics;
  assert.equal(diagnostic?.code, "missing-let");
  assert.equal(
    diagnostic?.message,
    "a binding with a type annotation must begin with 'let'; write `let x: T = ...`, or `x := ...` without the type",
  );
});

test("parser distinguishes inherent and trait implementation blocks", () => {
  const result = parse(conformanceBody("parse/valid/inherent-and-trait-impls"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.implementations[0]?.targetName, "User");
  assert.equal(result.program?.implementations[0]?.traitName, undefined);
  assert.equal(result.program?.implementations[1]?.traitName, "Read");
});

test("parser marks bare data members as embedded fields", () => {
  const result = parse(conformanceBody("parse/valid/embedded-data-member"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.data[1]?.fields[0]?.name, "Box");
  assert.equal(result.program?.data[1]?.fields[0]?.type.name, "Box[i32]");
  assert.equal(result.program?.data[1]?.fields[0]?.embedded, true);
});

test("parser retains explicit generic arguments and trailing callbacks", () => {
  const generic = parse(conformanceBody("parse/valid/explicit-generic-arguments-with-placeholder"));
  assert.deepEqual(generic.diagnostics, []);
  const binding = generic.program?.statements[0];
  if (binding?.kind === "binding" && binding.value.kind === "call") {
    assert.deepEqual(
      binding.value.typeArguments?.map((argument) => argument.name),
      ["_", "i32"],
    );
  } else {
    assert.fail("expected a generic call binding");
  }

  const trailing = parse(conformanceBody("parse/valid/trailing-callback-statement"));
  assert.deepEqual(trailing.diagnostics, []);
  const statement = trailing.program?.functions[0]?.body[0];
  assert.equal(statement?.kind === "expression" && statement.expression.kind, "call");
  if (statement?.kind === "expression" && statement.expression.kind === "call") {
    assert.equal(statement.expression.arguments[0]?.kind, "closure");
  }
});

test("parser retains explicit generic data arguments", () => {
  const result = parse(conformanceBody("parse/valid/explicit-generic-data-arguments"));
  assert.deepEqual(result.diagnostics, []);
  const binding = result.program?.statements[0];
  assert.equal(binding?.kind, "binding");
  if (binding?.kind !== "binding" || binding.value.kind !== "data") {
    assert.fail("expected a generic data binding");
  }
  assert.deepEqual(
    binding.value.typeArguments?.map((argument) => argument.name),
    ["i32"],
  );
});

test("tabs and inconsistent dedents are rejected by the lexer", () => {
  assert.equal(
    lex(conformanceBody("parse/invalid/tab-in-function-body")).diagnostics[0]?.code,
    "tab-whitespace",
  );
  assert.ok(
    lex(conformanceBody("parse/invalid/dedent-to-unused-column")).diagnostics.some(
      (item) => item.code === "invalid-dedent",
    ),
  );
});

test("lexer enforces reserved punctuation, escapes, and numeric separators", () => {
  assert.equal(
    lex(conformanceBody("parse/invalid/semicolon-after-binding")).diagnostics[0]?.code,
    "reserved-semicolon",
  );
  assert.equal(
    lex(conformanceBody("parse/invalid/double-numeric-separator")).diagnostics[0]?.code,
    "invalid-token",
  );
  assert.equal(
    lex(conformanceBody("parse/invalid/binary-literal-bad-digit")).diagnostics[0]?.code,
    "syntax-error",
  );
  assert.deepEqual(lex(conformanceBody("parse/valid/identifier-interpolation")).diagnostics, []);
  assert.equal(
    lex(conformanceBody("parse/valid/unicode-scalar-escape")).tokens.find(
      (token) => token.kind === "string",
    )?.value,
    "😀",
  );
});

test("parser retains string interpolation expressions; a prefixed string is a call", () => {
  const result = parse(conformanceBody("parse/valid/expression-interpolation"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "interpolated-string") {
    assert.deepEqual(
      statement.expression.segments.map((segment) => segment.kind),
      ["text", "expression", "text", "expression"],
    );
    assert.equal(
      statement.expression.segments[1]?.kind === "expression" &&
        statement.expression.segments[1].expression.kind,
      "name",
    );
    assert.equal(
      statement.expression.segments[3]?.kind === "expression" &&
        statement.expression.segments[3].expression.kind,
      "binary",
    );
  } else {
    assert.fail("expected an interpolated string");
  }
  const raw = parse(conformanceBody("parse/valid/raw-string-dollars"));
  const rawStatement = raw.program?.statements[0];
  assert.equal(rawStatement?.kind === "binding" && rawStatement.value.kind, "call");
});

test("parser rejects comparison chaining", () => {
  assert.equal(
    parse(conformanceBody("parse/invalid/comparison-chaining-with-names")).diagnostics[0]?.code,
    "comparison-chaining",
  );
});

test("parser retains named call labels and enforces argument ordering", () => {
  const result = parse(conformanceBody("parse/valid/named-call-arguments"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.statements[0];
  assert.equal(statement?.kind, "binding");
  if (statement?.kind === "binding" && statement.value.kind === "call") {
    assert.deepEqual(statement.value.argumentNames, ["width", "height"]);
  }
  assert.equal(
    parse(conformanceBody("parse/invalid/positional-after-named-argument")).diagnostics[0]?.code,
    "argument-order",
  );
});

test("parser retains vararg and positional spread markers", () => {
  const result = parse(conformanceBody("parse/valid/vararg-and-spread"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[0]?.type.name, "List[i32]");
  assert.equal(result.program?.functions[0]?.parameters[0]?.variadic, true);
  const statement = result.program?.statements[0];
  if (statement?.kind === "binding" && statement.value.kind === "call") {
    assert.deepEqual(statement.value.argumentSpreads, [true]);
  } else {
    assert.fail("expected a call binding");
  }
  assert.deepEqual(
    parse(conformanceBody("parse/valid/spread-before-named-argument")).diagnostics,
    [],
  );
  assert.equal(
    parse(conformanceBody("typing/invalid/positional-after-spread")).diagnostics[0]?.code,
    "nonfinal-positional-spread",
  );
});

test("parser retains function parameter defaults", () => {
  const result = parse(conformanceBody("parse/valid/parameter-defaults"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[1]?.default?.kind, "integer");
  assert.equal(result.program?.functions[0]?.parameters[2]?.default?.kind, "boolean");
  assert.equal(
    parse(conformanceBody("parse/invalid/vararg-with-default")).diagnostics[0]?.code,
    "syntax-error",
  );
});

test("parser retains a leading data copy-update spread", () => {
  const result = parse(conformanceBody("parse/valid/copy-update-spread"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.statements[0];
  assert.equal(statement?.kind, "binding");
  if (statement?.kind === "binding" && statement.value.kind === "data") {
    assert.equal(statement.value.spread?.kind, "name");
    assert.deepEqual(
      statement.value.fields.map((field) => field.name),
      ["name"],
    );
  } else {
    assert.fail("expected a data copy-update binding");
  }
  assert.equal(
    parse(conformanceBody("parse/invalid/copy-update-spread-not-first")).diagnostics[0]?.code,
    "syntax-error",
  );
});

test("parser retains named enum payload pattern labels", () => {
  const result = parse(conformanceBody("parse/valid/named-payload-pattern-labels"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "match") {
    const pattern = statement.expression.arms[0]?.pattern;
    assert.equal(pattern?.kind, "variant");
    if (pattern?.kind === "variant") {
      assert.deepEqual(pattern.bindingNames, ["right", "left"]);
      assert.deepEqual(pattern.bindings, ["b", "a"]);
    }
  } else {
    assert.fail("expected a match expression");
  }
});

test("parser retains nested Result payload patterns", () => {
  const result = parse(conformanceBody("parse/valid/nested-result-payload-pattern"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "match") {
    const pattern = statement.expression.arms[0]?.pattern;
    assert.equal(pattern?.kind, "variant");
    if (pattern?.kind === "variant") {
      assert.equal(pattern.variantName, "Err");
      assert.equal(pattern.payloadPatterns?.[0]?.kind, "variant");
    }
  } else {
    assert.fail("expected a match expression");
  }
});

test("parser retains literal enum payload patterns", () => {
  const result = parse(conformanceBody("parse/valid/literal-payload-pattern"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "match") {
    const pattern = statement.expression.arms[0]?.pattern;
    assert.equal(pattern?.kind, "variant");
    if (pattern?.kind === "variant") assert.equal(pattern.payloadPatterns?.[1]?.kind, "integer");
  } else {
    assert.fail("expected a match expression");
  }
});

test("parser retains shared enum fields, defaults, and variant results", () => {
  const result = parse(conformanceBody("parse/valid/shared-enum-fields"));
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(
    result.program?.enums[0]?.sharedFields.map((field) => field.name),
    ["code", "phrase"],
  );
  assert.equal(result.program?.enums[0]?.sharedFields[1]?.default?.kind, "string");
  assert.equal(result.program?.enums[0]?.variants[0]?.result?.kind, "call");
});

test("parser retains underscore tuple member selectors", () => {
  const result = parse(conformanceBody("parse/valid/numeric-member-selectors"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.statements[0];
  assert.equal(statement?.kind, "binding");
  if (statement?.kind === "binding" && statement.value.kind === "member") {
    assert.equal(statement.value.name, "_1");
    assert.equal(statement.value.receiver.kind, "member");
    if (statement.value.receiver.kind === "member")
      assert.equal(statement.value.receiver.name, "_0");
  } else {
    assert.fail("expected nested tuple member access");
  }
});

test("parser distinguishes grouped expressions from tuple types and literals", () => {
  const result = parse(conformanceBody("parse/valid/tuples-and-groups"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[0]?.type.name, "(i32,string)");
  assert.equal(result.program?.functions[0]?.result.name, "(i32,)");
  const body = result.program?.functions[0]?.body[0];
  assert.equal(body?.kind, "expression");
  if (body?.kind === "expression") assert.equal(body.expression.kind, "tuple");
  const empty = result.program?.statements[0];
  if (empty?.kind === "binding") assert.equal(empty.value.kind, "tuple");
  assert.equal(result.program?.statements[1]?.kind, "tuple-binding");
  const mutable = result.program?.statements[2];
  if (mutable?.kind === "tuple-binding") {
    assert.deepEqual(
      mutable.bindings.map((binding) => binding.name),
      ["name", "score"],
    );
    assert.equal(mutable.annotation?.name, "(string,i32)");
    assert.equal(mutable.mutable, true);
  }
});

test("documentation comments attach to AST declarations and members", () => {
  const source = conformanceBody("parse/valid/documentation-comments");
  const result = parse(source);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.data[0]?.doc, "A point.\nStored in two dimensions.");
  assert.equal(result.program?.data[0]?.fields[0]?.doc, "Horizontal coordinate.");
  assert.equal(result.program?.functions[0]?.doc, "Read one coordinate.");
  assert.equal(result.program?.functions[0]?.parameters[0]?.doc, "Point to inspect.");
  assert.equal(
    parse(conformanceBody("parse/invalid/doc-comment-without-target")).diagnostics[0]?.code,
    "doc-comment-without-target",
  );
});

test("parser retains top-level public visibility", () => {
  const result = parse(conformanceBody("parse/valid/public-declarations"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.traits[0]?.public, true);
  assert.equal(result.program?.data[0]?.public, true);
  assert.equal(result.program?.enums[0]?.public, true);
  assert.equal(result.program?.functions[0]?.public, true);
});

test("parser retains the test cases of a tests block", () => {
  const result = parse(conformanceBody("parse/valid/test-block-discard"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.tests[0]?.name, "checks a value");
  assert.equal(result.program?.tests[0]?.body[0]?.kind, "discard");
});

test("parser lowers multi-provider use to an ordered tuple", () => {
  const result = parse(conformanceBody("parse/valid/multi-provider-use"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "discard" && statement.value.kind === "tuple") {
    assert.deepEqual(
      statement.value.elements.map((element) =>
        element.kind === "provider-use" ? element.key : undefined,
      ),
      ["Clock", "Logger"],
    );
  } else {
    assert.fail("expected a provider tuple discard");
  }
});

test("parser retains single and grouped use declarations", () => {
  const result = parse(conformanceBody("parse/valid/single-and-grouped-use"));
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(
    result.program?.uses.map((declaration) => ({
      module: declaration.module,
      names: declaration.names,
    })),
    [
      { module: "std.task", names: [{ name: "block_on" }] },
      {
        module: "std.testing",
        names: [{ name: "assert" }, { name: "assert_equal", alias: "equal" }],
      },
    ],
  );
});

// A `mut` requirement key suggests dropping `mut`
// (spec/lang/11-requirements-and-suspension.md#r-req.mut.no-spelling).
test("parser suggests dropping mut from a requirement key", () => {
  const [diagnostic] = parse(conformanceBody("parse/invalid/mutable-requirement-key")).diagnostics;
  assert.equal(diagnostic?.code, "syntax-error");
  assert.match(diagnostic?.message ?? "", /drop `mut`/);
});
