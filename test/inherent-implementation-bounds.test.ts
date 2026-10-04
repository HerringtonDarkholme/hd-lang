import assert from "node:assert/strict";
import test from "node:test";

import type { Program } from "../src/ast.ts";
import { parse } from "../src/parser/index.ts";
import { check } from "../src/checker/program.ts";

function withStandardImplementation(source: string): Program {
  const parsed = parse(source, { standardLibrary: true });
  assert.deepEqual(parsed.diagnostics, []);
  assert.ok(parsed.program);
  return {
    ...parsed.program,
    implementations: parsed.program.implementations.map((implementation) => ({
      ...implementation,
      standard: true,
    })),
  };
}

test("a bounded standard inherent Map implementation keeps its key bounds", () => {
  const result = check(
    withStandardImplementation(`impl[K < Eq & Hash, V] Map[K, V]:
    pub fn bound_probe(self) -> i32: 1

fn probe[K < Eq & Hash, V](values: Map[K, V]) -> i32:
    values.bound_probe()
`),
  );

  assert.deepEqual(result.diagnostics, []);
  const method = result.program?.functions.find(
    (declaration) => declaration.name === "$inherent0.bound_probe",
  );
  assert.ok(method);
  assert.deepEqual(
    method.genericBounds.map((bound) => [bound.parameter, bound.traitName]),
    [
      ["K", "Eq"],
      ["K", "Hash"],
    ],
  );
});

test("an unbounded standard inherent Map implementation still fails its key bound", () => {
  const result = check(
    withStandardImplementation(`impl[K, V] Map[K, V]:
    pub fn invalid_probe(self) -> i32: 1
`),
  );

  assert.deepEqual(
    result.diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]),
    [
      [
        "unsatisfied-trait-bound",
        "type 'K' does not implement Eq and Hash, required by the bound on 'K' of 'Map'",
      ],
    ],
  );
});
