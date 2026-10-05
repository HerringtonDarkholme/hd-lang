import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function checkSource(source: string): [string, string][] {
  const result = analyze(source);
  return result.diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.message]);
}

const SETUP = `data P:
    x: i32

data Box[T < Display]:
    value: T
`;

test("a written application checks its declaration's bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
fn top(b: Box[P]) -> void:
    println("t")

pub fn main() -> void $ Console:
    println("done")
`),
    [
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Display, required by the bound on 'T' of 'Box'",
      ],
    ],
  );
});

test("a nested written application checks its declaration's bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
fn nested(bs: List[Box[P]]) -> void:
    println("n")

fn count(ms: List[Map[P, i32]]) -> void:
    println("c")

pub fn main() -> void $ Console:
    println("done")
`),
    [
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Display, required by the bound on 'T' of 'Box'",
      ],
      [
        "unsatisfied-trait-bound",
        "type 'P' does not implement Eq and Hash, required by the bound on 'K' of 'Map'",
      ],
    ],
  );
});

test("a written application accepts arguments that meet the bounds", () => {
  assert.deepEqual(
    checkSource(`${SETUP}
impl Display for P:
    fn to_string(self) -> string:
        "p"

fn forwarded[T < Display](b: Box[T]) -> void:
    println("f")

fn concrete(bs: List[Box[P]]) -> void:
    println("c")

pub fn main() -> void $ Console:
    println("done")
`),
    [],
  );
});
