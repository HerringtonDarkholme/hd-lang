import assert from "node:assert/strict";
import test from "node:test";

import type { DataField } from "../src/ast.ts";
import { analyze } from "../src/compiler.ts";
import type { SourceSpan } from "../src/diagnostics.ts";
import {
  derivedFieldDiagnostic,
  derivedFieldSpan,
  derivedImplementationSpan,
  DERIVED_IMPLEMENTATION_SPANS,
} from "../src/checker/derive-intrinsics.ts";

const ROWS = `trait Db

trait Clock

fn health() -> string $ Clock:
    "ok"

fn orders() -> string $ Db:
    "orders"

pub fn main() -> void $ Console:
    base := [health]
    plain := [base..., orders]
    println("\${plain.len()}")
    mixed := [base..., if true: orders else: orders]
    println("\${mixed.len()}")
    fns := [base..., fn() -> string: orders()]
    println("\${fns.len()}")
`;

test("O-08: a spread list accepts if and closure function elements", () => {
  const result = analyze(ROWS);
  assert.deepEqual(
    result.diagnostics.filter((diagnostic) => diagnostic.severity !== "warning"),
    [],
  );
});

const ARBITRARY = `use std.testing.Arbitrary

type Handler = fn(i32) -> i32

@derive(Arbitrary)
data Direct:
    f: fn(i32) -> i32

@derive(Arbitrary)
data ViaAlias:
    h: Handler

pub fn main() -> void $ Console:
    println("x")
`;

test("O-09: a derived Arbitrary error names a member typed through an alias", () => {
  const result = analyze(ARBITRARY);
  const errors = result.diagnostics.filter(
    (diagnostic) => diagnostic.code === "unsatisfied-trait-bound",
  );
  assert.equal(errors.length, 2);
  assert.match(errors[0]!.message, /member 'f' cannot be derived/);
  assert.match(errors[1]!.message, /member 'h' cannot be derived/);
});

function spanAt(offset: number): SourceSpan {
  return {
    start: { offset, line: 1, column: offset + 1 },
    end: { offset: offset + 1, line: 1, column: offset + 2 },
  };
}

function copied(span: SourceSpan): SourceSpan {
  return JSON.parse(JSON.stringify(span)) as SourceSpan;
}

test("O-11: derive diagnostics survive span copies", () => {
  const field = {
    name: "a",
    type: { name: "fn(i32) -> i32", span: spanAt(0) },
    span: spanAt(10),
  } as DataField;
  const registered = derivedFieldSpan(field, "Eq", "Pair");
  assert.deepEqual(
    derivedFieldDiagnostic("unsatisfied-trait-bound", "message", copied(registered)),
    {
      code: "derive-field-missing-trait",
      message: "field 'a' of 'Pair' does not implement Eq, which @derive(Eq) requires",
    },
  );
  const implementation = derivedImplementationSpan(spanAt(20));
  assert.equal(DERIVED_IMPLEMENTATION_SPANS.has(copied(implementation)), true);
});
