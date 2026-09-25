import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { lex } from "../src/lexer.ts";
import { parse } from "../src/parser/index.ts";
import { fixtureBody } from "./fixture.ts";

const CORE_PROGRAM = fixtureBody("frontend/00-core-program");

test("lexer emits layout tokens and source positions", () => {
  const result = lex(CORE_PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.tokens.filter((token) => token.kind === "indent").length, 4);
  assert.equal(result.tokens.filter((token) => token.kind === "dedent").length, 4);
  const choose = result.tokens.find((token) => token.text === "choose");
  assert.deepEqual(choose?.span.start, { offset: 3, line: 1, column: 4 });
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
  const result = parse(fixtureBody("frontend/04-parser-builds-value-producing-while-else"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  assert.equal(statement?.kind, "expression");
  if (statement?.kind === "expression" && statement.expression.kind === "while") {
    assert.equal(statement.expression.elseBody.length, 1);
  }
});

test("parser builds for loops with tuple bindings and else suites", () => {
  const result = parse(
    fixtureBody("frontend/05-parser-builds-for-loops-with-tuple-bindings-and-else-suites"),
  );
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
  const result = parse(
    fixtureBody("frontend/06-parser-lowers-named-local-functions-to-typed-closure-bindings"),
  );
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "binding") {
    assert.equal(statement.name, "add");
    assert.equal(statement.annotation?.name, "fn(i32)->i32");
    assert.equal(statement.value.kind, "closure");
  } else {
    assert.fail("expected a local closure binding");
  }
});

test("parser retains mutable permission types and field assignments", () => {
  const result = parse(
    fixtureBody("frontend/07-parser-retains-mutable-permission-types-and-field-assignments"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[0]?.type.name, "mut:User");
  assert.equal(result.program?.functions[0]?.result.name, "mut:User");
  assert.equal(result.program?.functions[0]?.body[0]?.kind, "field-assignment");
  assert.equal(
    parse(
      fixtureBody("frontend/07-parser-retains-mutable-permission-types-and-field-assignments-2"),
    ).program?.functions[0]?.body[0]?.kind,
    "index-assignment",
  );
});

test("parser represents mut self as a mutable Self receiver", () => {
  const result = parse(
    fixtureBody("frontend/08-parser-represents-mut-self-as-a-mutable-self-receiver"),
  );
  assert.deepEqual(result.diagnostics, []);
  const receiver = result.program?.traits[0]?.methods[0]?.parameters[0];
  assert.equal(receiver?.name, "self");
  assert.equal(receiver?.type.name, "mut:Self");

  assert.equal(
    parse(fixtureBody("frontend/08-parser-represents-mut-self-as-a-mutable-self-receiver-2"))
      .diagnostics[0]?.code,
    "expected-token",
  );
});

test("parser distinguishes inherent and trait implementation blocks", () => {
  const result = parse(
    fixtureBody("frontend/09-parser-distinguishes-inherent-and-trait-implementation-blocks"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.implementations[0]?.targetName, "User");
  assert.equal(result.program?.implementations[0]?.traitName, undefined);
  assert.equal(result.program?.implementations[1]?.traitName, "Read");
});

test("parser marks bare data members as embedded fields", () => {
  const result = parse(
    fixtureBody("frontend/10-parser-marks-bare-data-members-as-embedded-fields"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.data[1]?.fields[0]?.name, "Box");
  assert.equal(result.program?.data[1]?.fields[0]?.type.name, "Box[i32]");
  assert.equal(result.program?.data[1]?.fields[0]?.embedded, true);
});

test("parser retains explicit generic arguments and trailing callbacks", () => {
  const generic = parse(
    fixtureBody("frontend/11-parser-retains-explicit-generic-arguments-and-trailing-callbacks"),
  );
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

  const trailing = parse(
    fixtureBody("frontend/11-parser-retains-explicit-generic-arguments-and-trailing-callbacks-2"),
  );
  assert.deepEqual(trailing.diagnostics, []);
  const statement = trailing.program?.functions[0]?.body[0];
  assert.equal(statement?.kind === "expression" && statement.expression.kind, "call");
  if (statement?.kind === "expression" && statement.expression.kind === "call") {
    assert.equal(statement.expression.arguments[0]?.kind, "closure");
  }
});

test("parser retains explicit generic data arguments", () => {
  const result = parse(fixtureBody("frontend/12-parser-retains-explicit-generic-data-arguments"));
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
    lex(fixtureBody("frontend/13-tabs-and-inconsistent-dedents-are-rejected-by-the-lexer"))
      .diagnostics[0]?.code,
    "tab-whitespace",
  );
  assert.ok(
    lex(
      fixtureBody("frontend/13-tabs-and-inconsistent-dedents-are-rejected-by-the-lexer-2"),
    ).diagnostics.some((item) => item.code === "inconsistent-dedent"),
  );
});

test("lexer enforces reserved punctuation, escapes, and numeric separators", () => {
  assert.equal(
    lex(
      fixtureBody("frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators"),
    ).diagnostics[0]?.code,
    "reserved-semicolon",
  );
  assert.equal(
    lex(
      fixtureBody(
        "frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-2",
      ),
    ).diagnostics[0]?.code,
    "invalid-integer-literal",
  );
  assert.equal(
    lex(
      fixtureBody(
        "frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-3",
      ),
    ).diagnostics[0]?.code,
    "invalid-integer-literal",
  );
  assert.deepEqual(
    lex(
      fixtureBody(
        "frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-4",
      ),
    ).diagnostics,
    [],
  );
  assert.equal(
    lex(
      fixtureBody(
        "frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-5",
      ),
    ).diagnostics[0]?.code,
    "invalid-string-interpolation",
  );
  assert.equal(
    lex(
      fixtureBody(
        "frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-6",
      ),
    ).tokens.find((token) => token.kind === "string")?.value,
    "😀",
  );
});

test("parser retains string interpolation expressions and raw dollars", () => {
  const result = parse(
    fixtureBody("frontend/15-parser-retains-string-interpolation-expressions-and-raw-dollars"),
  );
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
  const raw = parse(
    fixtureBody("frontend/15-parser-retains-string-interpolation-expressions-and-raw-dollars-2"),
  );
  const rawStatement = raw.program?.statements[0];
  assert.equal(rawStatement?.kind === "binding" && rawStatement.value.kind, "string");
});

test("parser rejects comparison chaining", () => {
  assert.equal(
    parse(fixtureBody("frontend/16-parser-rejects-comparison-chaining")).diagnostics[0]?.code,
    "comparison-chaining",
  );
});

test("parser retains named call labels and enforces argument ordering", () => {
  const result = parse(
    fixtureBody("frontend/17-parser-retains-named-call-labels-and-enforces-argument-ordering"),
  );
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.statements[0];
  assert.equal(statement?.kind, "binding");
  if (statement?.kind === "binding" && statement.value.kind === "call") {
    assert.deepEqual(statement.value.argumentNames, ["width", "height"]);
  }
  assert.equal(
    parse(
      fixtureBody("frontend/17-parser-retains-named-call-labels-and-enforces-argument-ordering-2"),
    ).diagnostics[0]?.code,
    "argument-order",
  );
});

test("parser retains vararg and positional spread markers", () => {
  const result = parse(
    fixtureBody("frontend/18-parser-retains-vararg-and-positional-spread-markers"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[0]?.type.name, "fn(i32...)->i32");
  assert.equal(result.program?.functions[1]?.parameters[0]?.variadic, true);
  const statement = result.program?.statements[0];
  if (statement?.kind === "binding" && statement.value.kind === "call") {
    assert.deepEqual(statement.value.argumentSpreads, [true]);
  } else {
    assert.fail("expected a call binding");
  }
  assert.deepEqual(
    parse(fixtureBody("frontend/18-parser-retains-vararg-and-positional-spread-markers-2"))
      .diagnostics,
    [],
  );
  assert.equal(
    parse(fixtureBody("frontend/18-parser-retains-vararg-and-positional-spread-markers-3"))
      .diagnostics[0]?.code,
    "nonfinal-positional-spread",
  );
});

test("parser retains function parameter defaults", () => {
  const result = parse(fixtureBody("frontend/19-parser-retains-function-parameter-defaults"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.functions[0]?.parameters[1]?.default?.kind, "integer");
  assert.equal(result.program?.functions[0]?.parameters[2]?.default?.kind, "boolean");
  assert.equal(
    parse(fixtureBody("frontend/19-parser-retains-function-parameter-defaults-2")).diagnostics[0]
      ?.code,
    "vararg-default",
  );
});

test("parser retains a leading data copy-update spread", () => {
  const result = parse(fixtureBody("frontend/20-parser-retains-a-leading-data-copy-update-spread"));
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
    parse(fixtureBody("frontend/20-parser-retains-a-leading-data-copy-update-spread-2"))
      .diagnostics[0]?.code,
    "data-spread-position",
  );
});

test("parser retains named enum payload pattern labels", () => {
  const result = parse(fixtureBody("frontend/21-parser-retains-named-enum-payload-pattern-labels"));
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
  const result = parse(fixtureBody("frontend/22-parser-retains-nested-result-payload-patterns"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.functions[0]?.body[0];
  if (statement?.kind === "expression" && statement.expression.kind === "match") {
    const pattern = statement.expression.arms[0]?.pattern;
    assert.equal(pattern?.kind, "result-variant");
    if (pattern?.kind === "result-variant")
      assert.equal(pattern.payloadPatterns?.[0]?.kind, "variant");
  } else {
    assert.fail("expected a match expression");
  }
});

test("parser retains literal enum payload patterns", () => {
  const result = parse(fixtureBody("frontend/23-parser-retains-literal-enum-payload-patterns"));
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
  const result = parse(
    fixtureBody("frontend/24-parser-retains-shared-enum-fields-defaults-and-variant-results"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(
    result.program?.enums[0]?.sharedFields.map((field) => field.name),
    ["code", "phrase"],
  );
  assert.equal(result.program?.enums[0]?.sharedFields[1]?.default?.kind, "string");
  assert.equal(result.program?.enums[0]?.variants[0]?.result?.kind, "call");
});

test("parser retains decimal numeric member selectors", () => {
  const result = parse(fixtureBody("frontend/25-parser-retains-decimal-numeric-member-selectors"));
  assert.deepEqual(result.diagnostics, []);
  const statement = result.program?.statements[0];
  assert.equal(statement?.kind, "binding");
  if (statement?.kind === "binding" && statement.value.kind === "member") {
    assert.equal(statement.value.name, "1");
    assert.equal(statement.value.receiver.kind, "member");
    if (statement.value.receiver.kind === "member")
      assert.equal(statement.value.receiver.name, "0");
  } else {
    assert.fail("expected nested numeric member access");
  }
});

test("parser distinguishes grouped expressions from tuple types and literals", () => {
  const result = parse(
    fixtureBody(
      "frontend/26-parser-distinguishes-grouped-expressions-from-tuple-types-and-literals",
    ),
  );
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
  const source = fixtureBody(
    "frontend/27-documentation-comments-attach-to-ast-declarations-and-members-source",
  );
  const result = parse(source);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.data[0]?.doc, "A point.\nStored in two dimensions.");
  assert.equal(result.program?.data[0]?.fields[0]?.doc, "Horizontal coordinate.");
  assert.equal(result.program?.functions[0]?.doc, "Read one coordinate.");
  assert.equal(result.program?.functions[0]?.parameters[0]?.doc, "Point to inspect.");
  assert.equal(
    parse(fixtureBody("frontend/27-documentation-comments-attach-to-ast-declarations-and-members"))
      .diagnostics[0]?.code,
    "doc-comment-without-target",
  );
});

test("parser retains top-level public visibility", () => {
  const result = parse(fixtureBody("frontend/28-parser-retains-top-level-public-visibility"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.traits[0]?.public, true);
  assert.equal(result.program?.data[0]?.public, true);
  assert.equal(result.program?.enums[0]?.public, true);
  assert.equal(result.program?.functions[0]?.public, true);
});

test("parser retains named test blocks", () => {
  const result = parse(fixtureBody("frontend/29-parser-retains-named-test-blocks"));
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program?.tests[0]?.name, "checks a value");
  assert.equal(result.program?.tests[0]?.body[0]?.kind, "discard");
});

test("parser lowers multi-provider use to an ordered tuple", () => {
  const result = parse(
    fixtureBody("frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple"),
  );
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
  const result = parse(
    fixtureBody("frontend/31-parser-retains-single-and-grouped-use-declarations"),
  );
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
