import assert from "node:assert/strict";
import test from "node:test";

import { instantiate } from "../compiler.ts";
import type { HirFunction, HirLocal } from "../hir.ts";
import { shareCapturedLocals } from "./captured-cells.ts";
import { ZERO_SPAN } from "./generated-source.ts";

function checkedFunction(index: number, locals: readonly HirLocal[] = []): HirFunction {
  return {
    name: `checked${index}`,
    index,
    suspending: false,
    variadic: false,
    genericParameters: [],
    genericBounds: [],
    rowParameters: [],
    parameters: [],
    result: "i32",
    requirements: [],
    locals,
    body: [],
    span: ZERO_SPAN,
    synthetic: false,
    closure: false,
    captures: [],
  };
}

test("shared capture lookup uses explicit closure index rather than array position", () => {
  const source: HirLocal = {
    name: "count",
    type: "i32",
    index: 0,
    mutable: true,
    parameter: false,
    span: ZERO_SPAN,
  };
  const parent = checkedFunction(0, [source]);
  const closure: HirFunction = {
    ...checkedFunction(7),
    closure: true,
    captures: [{ source, fieldIndex: 3 }],
    body: [
      {
        kind: "expression",
        expression: {
          kind: "capture",
          closureIndex: 7,
          fieldIndex: 3,
          type: "i32",
          span: ZERO_SPAN,
        },
        span: ZERO_SPAN,
      },
    ],
  };
  const converted = shareCapturedLocals([parent], [closure]);
  const body = converted.closures[0]!.body[0]!;
  assert.ok(body.kind === "expression" && body.expression.kind === "cell-get");
  assert.equal(body.expression.cell.type, "cell:i32");
  assert.equal(converted.closures[0]!.captures[0]!.source, converted.functions[0]!.locals[0]);
});

test("cell conversion is idempotent and keeps distinct activations' local identities separate", () => {
  const first: HirLocal = {
    name: "count",
    type: "i32",
    index: 0,
    mutable: true,
    parameter: false,
    span: ZERO_SPAN,
  };
  const second: HirLocal = { ...first };
  const functions = [checkedFunction(0, [first]), checkedFunction(1, [second])];
  const closures = [first, second].map((source, position) => ({
    ...checkedFunction(7 + position),
    closure: true,
    captures: [{ source, fieldIndex: 0 }],
  }));
  const converted = shareCapturedLocals(functions, closures);
  assert.notEqual(converted.functions[0]!.locals[0], converted.functions[1]!.locals[0]);
  for (const position of [0, 1]) {
    assert.equal(
      converted.closures[position]!.captures[0]!.source,
      converted.functions[position]!.locals[0],
    );
    assert.equal(converted.functions[position]!.locals[0]!.type, "cell:i32");
  }
  assert.deepEqual(shareCapturedLocals(converted.functions, converted.closures), converted);
});

test("typed rewriting reaches dictionary bounds and match tests while leaving metadata intact", () => {
  const source: HirLocal = {
    name: "count",
    type: "i32",
    index: 0,
    mutable: true,
    parameter: false,
    span: ZERO_SPAN,
  };
  const read = { kind: "local", local: source, type: "i32", span: ZERO_SPAN } as const;
  const metadata = new Map([["code", "keep"]]);
  const fn = {
    ...checkedFunction(0, [source]),
    metadata,
    body: [
      {
        kind: "expression",
        span: ZERO_SPAN,
        expression: {
          kind: "match",
          type: "i32",
          span: ZERO_SPAN,
          subject: read,
          representation: "scalar",
          arms: [
            {
              span: ZERO_SPAN,
              bindings: [],
              tests: [{ literal: read }],
              body: [
                {
                  kind: "expression",
                  span: ZERO_SPAN,
                  expression: {
                    kind: "trait-dictionary",
                    traitIndex: 1,
                    type: "trait:Eq",
                    span: ZERO_SPAN,
                    dictionary: { bounds: [read], implementationIndex: 0, supertraits: [] },
                  },
                },
              ],
            },
          ],
        },
      },
    ],
  } as const;
  const closure = { ...checkedFunction(7), closure: true, captures: [{ source, fieldIndex: 0 }] };
  const converted = shareCapturedLocals([fn], [closure]);
  assert.equal(
    Object.getOwnPropertyDescriptor(converted.functions[0], "metadata")?.value,
    metadata,
  );
  const statement = converted.functions[0]!.body[0]!;
  assert.ok(statement.kind === "expression" && statement.expression.kind === "match");
  const match = statement.expression;
  assert.equal(match.subject.kind, "cell-get");
  assert.equal(match.arms[0]!.tests![0]!.literal!.kind, "cell-get");
  const dictionaryStatement = match.arms[0]!.body[0]!;
  assert.ok(
    dictionaryStatement.kind === "expression" &&
      dictionaryStatement.expression.kind === "trait-dictionary",
  );
  assert.equal(dictionaryStatement.expression.dictionary.bounds[0]!.kind, "cell-get");
});

test("nested mutable captures share storage after an activation ends and across suspension", async () => {
  const source = `fn child!(value: i32) -> i32: value

fn make!() -> fn!() -> i32:
    let count: i32 = 40
    make_inner := fn() -> fn!() -> i32:
        fn!() -> i32:
            count = count + 1
            child!(count)
    next := make_inner()
    count = count + 1
    next

fn main!() -> i32:
    next := make!()
    first := next!()
    next!()
`;
  for (const pending of [undefined, (_index: number, poll: number) => poll === 1]) {
    const { instance } = await instantiate(source, { pending });
    assert.equal((instance.exports.main as CallableFunction)(), 43);
  }
});

test("loop activations retain independent mutable cells while sibling closures share each cell", async () => {
  const source = `fn child!(value: i32) -> i32: value

fn main!() -> i32:
    let readers: mut List[fn() -> i32] = []
    let writers: mut List[fn!() -> i32] = []
    for seed in [10, 20]:
        let count: i32 = seed
        readers.append(fn() -> i32: count)
        writers.append(fn!() -> i32:
            count = count + 1
            child!(count)
        )
    left := writers[0]
    right := writers[1]
    _ := left!()
    _ := right!()
    _ := left!()
    read_left := readers[0]
    read_right := readers[1]
    read_left() + read_right()
`;
  for (const pending of [undefined, (_index: number, poll: number) => poll === 1]) {
    const { instance } = await instantiate(source, { pending });
    assert.equal((instance.exports.main as CallableFunction)(), 33);
  }
});
