import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

test("known mutable field types contextualize empty collection literals", () => {
  const source = `data Walk:
    state: mut Map[string, i32]
    path: mut List[i32]

pub fn main() -> void:
    let mut walk = Walk { state: {}, path: [] }
    walk.state["seen"] = 1
    walk.path.append(2)
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});

test("explicit data arguments contextualize empty mutable fields", () => {
  const source = `data Box[T]:
    items: mut List[T]

pub fn main() -> void:
    _ := Box::[i32] { items: [] }
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});

test("mutable generic data context infers nested data arguments", () => {
  const source = `data Box[T]:
    items: mut List[T]

data Outer:
    box: mut Box[i32]

pub fn main() -> void:
    let mut outer = Outer { box: Box { items: [] } }
    outer.box.items.append(1)
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});

test("inference-only field context reaches empty literals through control flow", () => {
  const source = `data Walk:
    state: mut Map[string, i32]

pub fn main() -> void:
    let mut walk = Walk { state: if true: {} else: {} }
    walk.state["seen"] = 1
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});

test("an unresolved field type does not guess an empty literal's element type", () => {
  const source = `data Box[T]:
    items: mut List[T]

pub fn main() -> void:
    _ := Box { items: [] }
`;

  assert.deepEqual(
    analyze(source).diagnostics.map(({ code }) => code),
    ["cannot-infer-type"],
  );
});

test("a readonly value can still initialize a direct mutable field", () => {
  const source = `data Walk:
    state: mut Map[string, i32]

pub fn main() -> void:
    let state: Map[string, i32] = {}
    _ := Walk { state: state }
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});

test("an inference hint does not require nested data to be mutable", () => {
  const source = `data Inner:
    values: mut List[i32]

data Outer:
    inner: mut Inner

pub fn main() -> void:
    let values: List[i32] = []
    _ := Outer { inner: Inner { values: values } }
    let mut outer = Outer { inner: Inner { values: [] } }
    outer.inner.values.append(1)
`;

  assert.deepEqual(analyze(source).diagnostics, []);
});
