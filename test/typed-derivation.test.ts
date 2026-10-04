import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

// Typed derivation (spec/lang/14-annotations.md#typed-derivation) with i32 members,
// so the prototype's lowering runs end to end.

const LIBRARY = `use std.structure.{Structure, Field, Variant, Members, Key, Walker, Describer, Source}

data Rename:
    name: string

fn rename(name: string) -> Rename:
    Rename { name: name }

trait Show:
    fn show(self) -> string

trait Count:
    fn count() -> i32

trait Fill:
    fn fill() -> Self

impl Show for i32:
    fn show(self) -> string:
        "$self"

impl Show for string:
    fn show(self) -> string:
        self

data Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + v.info.name + "("
        .Ok(())

    fn member[F < Show](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        name := match h.info.facts.find::[Rename]():
            .Some(found) => found.name
            .None => h.info.name
        self.out = self.out + name + "=" + value.show() + ";"
        .Ok(())

data Counter:
    total: i32

impl[S] Describer[S] for Counter:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        self.total = self.total + 1
        .Ok(())

data Filler:
    count: i32

impl[S] Source[S] for Filler:
    type Error = string

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], string]:
        .Ok(choices[choices.len() - 1])

    fn next(mut self, members: Members[S]) -> Result[Key[S], string]:
        .Ok(members.end())

    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, string]:
        .Err("no input")

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, string]:
        match h.default():
            .Some(value) => .Ok(value)
            .None => .Err("missing")

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let w: mut Shower = Shower { out: "" }
        _ := Structure::walk(self, w)
        w.out + ")"

impl[T] Count for T by Structure:
    fn count() -> i32:
        let d: mut Counter = Counter { total: 0 }
        _ := T::describe(d)
        d.total

impl[T] Fill for T by Structure:
    fn fill() -> T:
        let s: mut Filler = Filler { count: 0 }
        match T::build(s):
            .Ok(value) => value
            .Err(message) => panic(message)
`;

test("typed derivation reports its diagnostics at the opt-in", () => {
  const codes = (program: string): string[] =>
    analyze(`${LIBRARY}\n${program}`).diagnostics.map((diagnostic) => diagnostic.code);
  assert.deepEqual(codes("@derive(Show)\ndata Flag:\n    on: bool\n"), ["member-not-derivable"]);
  assert.deepEqual(codes("@derive(Missing)\ndata Flag:\n    on: i32\n"), ["underivable-trait"]);
  assert.deepEqual(
    codes("data Flag:\n    on: i32\n\nimpl Show for Flag by Structure:\n    off = []\n"),
    ["unknown-annotation-member"],
  );
  assert.deepEqual(
    codes("data Flag:\n    on: i32\n\nimpl Show for Flag by Structure:\n    on = pass\n"),
    ["omitted-member-without-default"],
  );
  assert.deepEqual(codes("@derive(Show)\nfn run() -> void:\n    pass\n"), [
    "decorator-not-annotator",
  ]);
});
