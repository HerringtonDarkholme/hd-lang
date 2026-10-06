import assert from "node:assert/strict";
import test from "node:test";

import type { DataField } from "../src/ast.ts";
import { analyze, instantiate } from "../src/compiler.ts";
import type { SourceSpan } from "../src/diagnostics.ts";
import {
  derivedFieldDiagnostic,
  derivedFieldSpan,
  derivedImplementationSpan,
  DerivedOrigins,
  isDerivedImplementation,
  registerDerivedOrigins,
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

test("O-08: the rechecked spread list runs and prints each length once", async () => {
  const lines: string[] = [];
  const { instance, compilation } = await instantiate(ROWS, {
    console: (text) => lines.push(text),
  });
  const main = compilation.hir.functions.find(({ entry }) => entry)!;
  (instance.exports[main.name] as CallableFunction)(
    ...main.requirements.map((requirement) => ({ requirement })),
  );
  assert.deepEqual(lines, ["2", "2", "2"]);
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
    (diagnostic) => diagnostic.code === "member-not-derivable",
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
  const origins = new DerivedOrigins();
  const program = {};
  registerDerivedOrigins(program, origins);
  const registered = derivedFieldSpan(origins, field, "Eq", "Pair");
  assert.deepEqual(
    derivedFieldDiagnostic(program, "unsatisfied-trait-bound", "message", copied(registered)),
    {
      code: "derive-field-missing-trait",
      message: "field 'a' of 'Pair' does not implement Eq, which @derive(Eq) requires",
    },
  );
  const implementation = derivedImplementationSpan(origins, spanAt(20));
  assert.equal(isDerivedImplementation(program, copied(implementation)), true);
  // Another program's lookups never see these origins.
  assert.equal(isDerivedImplementation({}, copied(implementation)), false);
});

// A derived field's diagnostic origin belongs to its own compilation: the
// next program's error at the same position keeps its own code and message.
const DERIVED_HASH = `@derive(Eq)
data Opq: pass
@derive(Eq, Hash)
data Status:
    code: Opq
`;

const PLAIN_MISMATCH = `# Type mismatch here
#
#
fn f(small: i16, b: i32) -> i32:
    1 + small + b
`;

test("B6: derive origins do not leak into the next compilation", () => {
  const errors = (source: string) =>
    analyze(source).diagnostics.filter((diagnostic) => diagnostic.severity !== "warning");
  const derived = errors(DERIVED_HASH);
  assert.equal(derived.length, 1);
  assert.equal(derived[0]!.code, "derive-field-missing-trait");
  assert.match(derived[0]!.message, /field 'code' of 'Status' does not implement Hash/);
  const plain = errors(PLAIN_MISMATCH);
  assert.equal(plain.length, 1);
  // The two programs report at the same line, column, and offsets.
  assert.deepEqual(plain[0]!.span, derived[0]!.span);
  assert.equal(plain[0]!.code, "type-mismatch");
  assert.match(plain[0]!.message, /operands have types i16 and i32/);
});
