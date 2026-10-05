import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function onlyDiagnostic(source: string): { readonly code: string; readonly message: string } {
  const diagnostics = analyze(source).diagnostics;
  assert.equal(diagnostics.length, 1);
  return diagnostics[0]!;
}

test("unknown-method diagnostics render mutable receiver types as source text", () => {
  const diagnostic = onlyDiagnostic(`data Holder:
    values: mut List[i32]

pub fn main() -> void:
    let mut holder = Holder { values: [] }
    holder.values.missing()
`);

  assert.equal(diagnostic.code, "unknown-method");
  assert.equal(diagnostic.message, "type 'mut List[i32]' has no supported method 'missing'");
});

test("mutable-parameter diagnostics render mutable types as source text", () => {
  const diagnostic = onlyDiagnostic(`fn take(value: mut List[char]) -> void:
    pass

pub fn main() -> void:
    let value: List[char] = []
    take(value)
`);

  assert.equal(diagnostic.code, "readonly-argument-to-mutable-parameter");
  assert.equal(
    diagnostic.message,
    "readonly argument 'List[char]' cannot satisfy mutable parameter 'mut List[char]'",
  );
});

test("mutable-upgrade diagnostics render target types as source text", () => {
  const diagnostic = onlyDiagnostic(`data Box:
    value: i32

pub fn main() -> void:
    let readonly: Box = Box { value: 1 }
    let mutable: mut Box = readonly
`);

  assert.equal(diagnostic.code, "mutable-upgrade");
  assert.equal(diagnostic.message, "readonly type 'Box' cannot be upgraded to 'mut Box'");
});

test("mismatch diagnostics render nested mutable types as source text", () => {
  const diagnostic = onlyDiagnostic(`data Box:
    value: i32

fn take(values: List[mut Box]) -> void:
    pass

pub fn main() -> void:
    let values: List[Box] = []
    take(values)
`);

  assert.equal(diagnostic.code, "type-mismatch");
  assert.equal(diagnostic.message, "expected List[mut Box], found List[Box]");
});

test("generic argument join conflicts render mutable types as source text", () => {
  const diagnostic = onlyDiagnostic(`data Box:
    value: i32

fn same[T](left: T, right: T) -> void:
    pass

pub fn main() -> void:
    let mutable: List[mut Box] = []
    let readonly: List[Box] = []
    same(mutable, readonly)
`);

  assert.equal(diagnostic.code, "type-mismatch");
  assert.equal(
    diagnostic.message,
    "arguments of types 'List[mut Box]' and 'List[Box]' both solve 'T' of 'same', and inference never widens a number; convert one argument to the other's type",
  );
});

test("implementation diagnostics render mutable target arguments as source text", () => {
  const diagnostic = onlyDiagnostic(`data Box:
    value: i32

data G[T]:
    value: T

impl G[mut Box]:
    fn x(self) -> void:
        pass

impl G[mut Box]:
    fn x(self) -> void:
        pass
`);

  assert.equal(diagnostic.code, "duplicate-inherent-member");
  assert.equal(
    diagnostic.message,
    "inherent method 'G[mut Box].x' is declared more than once for unifying targets",
  );
});

test("no-common-type diagnostics render trait values without the trait prefix", () => {
  const diagnostic = onlyDiagnostic(`trait Named:
    fn name(self) -> string

trait Shown < Named:
    fn show(self) -> string

trait Tagged < Named:
    fn tag(self) -> string

fn mix(shown: Shown, tagged: Tagged) -> void:
    items := [shown, tagged]
`);
  assert.equal(diagnostic.code, "no-common-type");
  assert.equal(diagnostic.message, "list elements have no common type: Shown, Tagged");
});

test("provider diagnostics render mutable key arguments as source text", () => {
  const diagnostic = onlyDiagnostic(`trait K[T]:
    fn marker(self) -> void

data Box:
    value: i32

pub fn main() -> void:
    _ := $.use(K[mut Box])
`);

  assert.equal(diagnostic.code, "missing-requirement");
  assert.equal(diagnostic.message, "provider 'K[mut Box]' is not available in the current context");
});
