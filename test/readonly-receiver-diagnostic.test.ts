import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function onlyDiagnostic(source: string): { readonly code: string; readonly message: string } {
  const diagnostics = analyze(source).diagnostics;
  assert.equal(diagnostics.length, 1);
  return diagnostics[0]!;
}

test("readonly generic inherent receiver names its binding and concrete type", () => {
  const diagnostic = onlyDiagnostic(`fn bad(values: List[i32]) -> void:
    values.push(1)
`);

  assert.equal(diagnostic.code, "mutable-receiver-required");
  assert.equal(
    diagnostic.message,
    "method 'push' takes mut self, but binding 'values' has readonly type 'List[i32]'",
  );
});

test("readonly user-inherent receiver names its parameter binding", () => {
  const diagnostic = onlyDiagnostic(`data Counter:
    value: i32

impl Counter:
    fn bump(mut self) -> void:
        self.value = self.value + 1

fn bad(counter: Counter) -> void:
    counter.bump()
`);

  assert.equal(diagnostic.code, "mutable-receiver-required");
  assert.equal(
    diagnostic.message,
    "method 'bump' takes mut self, but binding 'counter' has readonly type 'Counter'",
  );
});

test("readonly trait-method receiver names its binding", () => {
  const diagnostic = onlyDiagnostic(`trait Bump:
    fn bump(mut self) -> void

data Counter:
    value: i32

impl Bump for Counter:
    fn bump(mut self) -> void:
        self.value = self.value + 1

fn bad(counter: Counter) -> void:
    counter.bump()
`);

  assert.equal(diagnostic.code, "mutable-receiver-required");
  assert.equal(
    diagnostic.message,
    "method 'bump' takes mut self, but binding 'counter' has readonly type 'Counter'",
  );
});

test("readonly bound-method receiver names its generic binding", () => {
  const diagnostic = onlyDiagnostic(`trait Bump:
    fn bump(mut self) -> void

fn bad[T < Bump](value: T) -> void:
    value.bump()
`);

  assert.equal(diagnostic.code, "mutable-receiver-required");
  assert.equal(
    diagnostic.message,
    "method 'bump' takes mut self, but binding 'value' has readonly type 'T'",
  );
});

test("readonly field path does not blame its mutable root binding", () => {
  const diagnostic = onlyDiagnostic(`data Child:
    name: string

impl Child:
    fn rename(mut self, name: string) -> void:
        self.name = name

data Parent:
    child: Child

fn bad(parent: mut Parent) -> void:
    parent.child.rename("new")
`);

  assert.equal(diagnostic.code, "mutable-receiver-required");
  assert.equal(
    diagnostic.message,
    "method 'rename' takes mut self, but its receiver has readonly type 'Child'",
  );
});
