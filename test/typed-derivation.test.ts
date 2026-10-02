import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";

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
        .Ok()

    fn member[F < Show](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        name := match h.info.facts.find::[Rename]():
            .Some(found) => found.name
            .None => h.info.name
        self.out = self.out + name + "=" + value.show() + ";"
        .Ok()

data Counter:
    total: i32

impl[S] Describer[S] for Counter:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok()

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        self.total = self.total + 1
        .Ok()

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

async function run(program: string): Promise<unknown> {
  const { instance } = await instantiate(`${LIBRARY}\n${program}`);
  return (instance.exports.main as CallableFunction)();
}

test("a derived walk passes each member with its facts", async () => {
  const result = await run(`
@derive(Show, Count)
data Point:
    x: i32
    @rename("why")
    y: i32

fn main() -> i32:
    if Point { x: 1, y: 2 }.show() == "Point(x=1;why=2;)" && Point::count() == 2: 1 else: 0
`);
  assert.equal(result, 1);
});

test("a derivation block's member lines edit only its own derivation", async () => {
  const result = await run(`
data Cache: pass

data Order:
    id: i32
    total: i32
    cache: Cache = Cache {}

impl Show for Order by Structure:
    total = [rename("sum")]
    cache = pass

@derive(Count)
data Other:
    id: i32

fn main() -> i32:
    if Order { id: 1, total: 5 }.show() == "Order(id=1;sum=5;)": 1 else: 0
`);
  assert.equal(result, 1);
});

test("enum, generic, and embedded targets walk their members", async () => {
  const result = await run(`
@derive(Show)
data Stamp:
    at: i32

@derive(Show)
data Post:
    Stamp
    title: string

@derive(Show)
data Pair[A]:
    left: A
    right: A

@derive(Show)
enum Mode:
    Off
    On(level: i32)
    Raw(string)

fn main() -> i32:
    post := Post { Stamp: ...Stamp { at: 3 }, title: "t" }
    ok := (
        post.show() == "Post(Stamp=Stamp(at=3;);title=t;)" &&
        Pair { left: 1, right: 2 }.show() == "Pair(left=1;right=2;)" &&
        Mode.On(4).show() == "On(level=4;)" &&
        Mode.Raw("r").show() == "Raw(_0=r;)" &&
        Mode.Off.show() == "Off()"
    )
    if ok: 1 else: 0
`);
  assert.equal(result, 1);
});

test("a derived build fills members from their defaults", async () => {
  const result = await run(`
@derive(Fill)
data Settings:
    level: i32 = 7
    name: string = "n"

fn main() -> i32:
    let settings: Settings = Settings::fill()
    settings.level
`);
  assert.equal(result, 7);
});

test("@derive(Eq) compares data and enum members", async () => {
  const result = await run(`
@derive(Eq)
enum Color:
    Red
    Custom(string)

@derive(Eq)
data Paint:
    color: Color
    coats: i32

fn main() -> i32:
    same := Paint { color: Color.Custom("a"), coats: 2 } == Paint { color: Color.Custom("a"), coats: 2 }
    different := Color.Red == Color.Custom("a")
    if same && !different: 1 else: 0
`);
  assert.equal(result, 1);
});

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

test("@derive(Debug) on a newtype needs its base type's Debug and applies it", async () => {
  const codes = (program: string): string[] =>
    analyze(program).diagnostics.map((diagnostic) => diagnostic.code);
  assert.deepEqual(codes("data Opaque: pass\n\n@derive(Debug)\ntype Wrapped(Opaque)\n"), [
    "derive-field-missing-trait",
  ]);
  const { instance } = await instantiate(
    '@derive(Debug)\ntype Meters(i64)\n\nfn main() -> i32:\n    if debug(Meters(3)) == "3": 1 else: 0\n',
  );
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});
