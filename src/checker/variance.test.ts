import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";
import { parse } from "../parser/index.ts";
import { withDistinctMethodBinders } from "./generic-method-scope.ts";
import { renameScopeType } from "./generic-scope-types.ts";

const producer = "data Box[+T]:\n    value: T\n";
const consumer = "data Consumer[-T]:\n    consume: fn(T) -> void\n";

function diagnostics(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

test("distinct method binders preserve runtime arguments, dictionaries and captured values", async () => {
  const source = `use std.num.Num
${producer}
impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T: value
    fn zero[T < Num](self) -> T: T::zero()
    fn text[T < Display](self, value: T) -> fn() -> string:
        let T = value
        fn() -> string: T::to_string()
fn main() -> i32:
    box := Box { value: "receiver" }
    callback := box.text(42)
    if callback() == "42": box.echo(42) + box.zero::[i32]() else: 0
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("readonly public method inputs participate in nominal variance", () => {
  const source = `${producer}
impl[U] Box[U]:
    pub fn consume(self, value: U) -> void:
        ()
`;
  const result = analyze(source).diagnostics;
  assert.deepEqual(
    result.map((diagnostic) => diagnostic.code),
    ["invalid-variance"],
  );
  assert.equal(result[0]?.message, "'+U' occurs in a negative position");
  assert.equal(result[0]?.span.start.line, 5);
});

test("readonly public method results participate in nominal variance", () => {
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    pub fn produce(self, factory: fn() -> U) -> U:
        factory()
`),
    ["invalid-variance"],
  );
});

test("nested function parameter polarity reverses rather than becoming invariant", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn visit(self, visitor: fn(U) -> void) -> void:
        visitor(self.value)
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn produce(self, factory: fn() -> U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("mutable method signature positions are invariant", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume(self, value: mut U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("invariant method signature containers cannot hide variance violations", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume(self, value: U?) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("method generic binders do not capture implementation parameters in Self", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T:
        value
    pub fn copy[T](self) -> Self:
        self
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    pub fn consume[T](self, other: Self) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("nested implementation targets compose their declared variance signs", () => {
  assert.deepEqual(
    diagnostics(`${producer}
${consumer}
impl[U] Box[Consumer[U]]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
  assert.deepEqual(
    diagnostics(`${producer}
${consumer}
impl[U] Box[Consumer[U]]:
    pub fn produce(self, factory: fn() -> U) -> U:
        factory()
`),
    ["invalid-variance"],
  );
});

test("an invariant implementation target does not impose signed method constraints", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U?]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("opposing target signs constrain a shared parameter to equality", () => {
  assert.deepEqual(
    diagnostics(`data Pair[+A, -B]:
    value: A
    consume: fn(B) -> void

impl[U] Pair[U, U]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("trait arguments in method signatures retain their invariant polarity", () => {
  assert.deepEqual(
    diagnostics(`${producer}
trait Sink[T]:
    fn consume(self, value: T) -> void

impl[U] Box[U]:
    pub fn accept(self, sink: Sink[U]) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("a separate trait implementation does not change nominal variance", () => {
  assert.deepEqual(
    diagnostics(`${producer}
trait Sink[T]:
    fn consume(self, value: T) -> void

impl[U] Sink[U] for Box[U]:
    fn consume(self, value: U) -> void:
        ()
`),
    [],
  );
});

test("enum inherent methods are included in the readonly public surface", () => {
  assert.deepEqual(
    diagnostics(`enum Choice[+T]:
    Some(value: T)

impl[U] Choice[U]:
    pub fn consume(self, value: U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("suspending inherent methods have the same variance obligations", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn consume!(self, value: U) -> void:
        ()
`),
    ["invalid-variance"],
  );
});

test("associated construction and mutable receiver signatures are not readonly instance views", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    pub fn new(value: U) -> Self:
        Box::[U] { value: value }
    pub fn set(mut self, value: U) -> void:
        self.value = value
`),
    [],
  );
});

test("a public method cannot infer its result past the variance check", () => {
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    pub fn produce(self, factory: fn() -> U):
        factory()
`),
    ["missing-result-type"],
  );
});

test("private readonly inherent methods participate in declared variance", () => {
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    fn consume(self, value: U) -> void:
        ()
`),
    ["invalid-variance"],
  );
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    fn produce(self, factory: fn() -> U) -> U:
        factory()
`),
    ["invalid-variance"],
  );
});

test("inferred private inherent results cannot bypass variance checking", () => {
  assert.deepEqual(
    diagnostics(`${consumer}
impl[U] Consumer[U]:
    fn produce(self, factory: fn() -> U):
        factory()
`),
    ["invalid-variance"],
  );
  assert.deepEqual(
    diagnostics(`${producer}
impl[U] Box[U]:
    fn produce(self):
        self.value
`),
    [],
  );
});

test("shadowed method binders cannot erase Self-derived invariant inferred results", () => {
  for (const binder of ["T", "V"]) {
    for (const body of [
      "Wrap { value: self.consume }",
      "let mut values = [self.consume]\n        values",
    ]) {
      const result = analyze(`data Wrap[T]:
    value: T
${consumer}
impl[T] Consumer[T]:
    fn hidden[${binder}](self):
        ${body}
`).diagnostics;
      assert.deepEqual(
        result.map((d) => d.code),
        ["invalid-variance"],
      );
      assert.equal(result[0]!.message, "'-T' occurs in an invariant position");
    }
  }
});

test("method-owned inferred and callable results remain independent of the receiver binder", () => {
  for (const visibility of ["", "pub "]) {
    assert.deepEqual(
      diagnostics(`${producer}
impl[T] Box[T]:
    ${visibility}fn echo[T](self, value: T) -> T: value
    ${visibility}fn callback[T](self, value: T) -> fn() -> T:
        fn() -> T: value
`),
      [],
    );
  }
  assert.deepEqual(
    diagnostics(`${producer}
impl[T] Box[T]:
    fn echo[T](self, value: T): value
    fn callback[T](self, value: T):
        fn() -> T: value
    fn receiver[T](self): self
`),
    [],
  );
});

test("method binder normalization preserves defaults, bounds and nested declaration scope", () => {
  const parsed = parse(`impl[T] Box[T]:
    fn inspect[T < Display, U = T](self, value: T):
        data Inner[T]:
            value: T
        data Captured[U = T]:
            value: T
        trait Nested[T]:
            fn read(self) -> T
        fn nested(value: T) -> T: value
        nested(value)
`);
  assert.deepEqual(parsed.diagnostics, []);
  const normalized = withDistinctMethodBinders(parsed.program!);
  const method = normalized.implementations[0]!.methods[0]!;
  const identity = method.genericParameters[0]!;
  assert.match(identity, /^%method\d+\.T$/u);
  assert.equal(method.genericDefaults!.U!.name, identity);
  assert.equal(method.genericBounds[0]!.parameter, identity);
  assert.equal(method.parameters[1]!.type.name, identity);
  assert.equal(method.span, parsed.program!.implementations[0]!.methods[0]!.span);
  const [inner, captured, nested, local] = method.body!;
  assert.ok(inner?.kind === "local-declaration" && inner.declaration.kind === "data");
  assert.equal(inner.declaration.fields[0]!.type.name, "T");
  assert.ok(captured?.kind === "local-declaration" && captured.declaration.kind === "data");
  assert.equal(captured.declaration.fields[0]!.type.name, identity);
  assert.equal(captured.declaration.genericDefaults!.U!.name, identity);
  assert.ok(nested?.kind === "local-declaration" && nested.declaration.kind === "trait");
  assert.equal(nested.declaration.methods[0]!.result.name, "T");
  assert.ok(local?.kind === "binding" && local.value.kind === "closure");
  assert.equal(local.value.parameters[0]!.type!.name, identity);
  assert.equal(local.value.result!.name, identity);
});

test("structural binder renaming retains type heads and labels while visiting rows and projections", () => {
  const names = new Map([["T", "%method0.T"]]);
  assert.equal(
    renameScopeType("fn(T)->Result[T,Box[T]]$Db[T]", names),
    "fn(%method0.T)->Result[%method0.T,Box[%method0.T]]$Db[%method0.T]",
  );
  assert.equal(
    renameScopeType("Bound[T=T,Out=T::Item]", names),
    "Bound[T=%method0.T,Out=%method0.T::Item]",
  );
  assert.equal(renameScopeType("(mut:T)?", names), "(mut:%method0.T)?");
});

test("a local nominal shadows a method binder from its declaration point without leaking out", () => {
  const result = analyze(`${producer}
impl[T] Box[T]:
    fn work[T](self) -> void:
        data T:
            n: i32
        let x: T = T { n: 1 }
        ()
`).diagnostics;
  assert.deepEqual(
    result.map((d) => d.code),
    ["unused-local-binding"],
  );
});

test("qualified type owners rename binders but preserve unrelated owners", () => {
  const parsed = parse(`impl[T] Box[T]:
    fn work[T](self):
        T::make()
        Module::make()
`);
  assert.deepEqual(parsed.diagnostics, []);
  const method = withDistinctMethodBinders(parsed.program!).implementations[0]!.methods[0]!;
  for (const [index, owner] of [
    [0, method.genericParameters[0]!],
    [1, "Module"],
  ] as const) {
    const statement = method.body![index]!;
    assert.ok(statement.kind === "expression" && statement.expression.kind === "call");
    assert.ok(statement.expression.callee.kind === "qualified-name");
    assert.equal(
      statement.expression.callee.genericTypeOwner ?? statement.expression.callee.owner,
      owner,
    );
  }
});

test("internal method identities retain Unicode binder spellings in diagnostics", () => {
  const result = analyze(`data Box[+Τ]:
    value: Τ
impl[Τ] Box[Τ]:
    fn echo[Τ](self, value: Τ) -> i32: value
`).diagnostics;
  assert.deepEqual(
    result.map((d) => d.code),
    ["type-mismatch"],
  );
  assert.match(result[0]!.message, /generic:Τ/u);
  assert.doesNotMatch(result[0]!.message, /%method/u);
});

test("qualified owner resolution distinguishes type binders from lexical values", () => {
  assert.deepEqual(
    diagnostics(`use std.num.Num
${producer}
impl[T] Box[T]:
    fn zero[T < Num](self) -> T: T::zero()
    fn text[T < Display](self, value: T) -> string:
        let T = value
        T::to_string()
    fn closure[T < Display](self, value: T) -> fn() -> string:
        let T = value
        fn() -> string: T::to_string()
    fn optional[T < Display](self, value: T?) -> string:
        match value:
            .Some(T) => T::to_string()
            .None => ""
`),
    [],
  );
});

test("nested closure type scopes do not change later method annotations", () => {
  const parsed = parse(`impl[T] Box[T]:
    fn work[T](self, value: T):
        nested := fn() -> void:
            type T = i32
            let local: T = 1
            ()
        let later: T = value
        later
`);
  assert.deepEqual(parsed.diagnostics, []);
  const method = withDistinctMethodBinders(parsed.program!).implementations[0]!.methods[0]!;
  const nested = method.body![0]!;
  assert.ok(nested.kind === "binding" && nested.value.kind === "closure");
  const local = nested.value.body[1]!;
  assert.ok(local.kind === "binding");
  assert.equal(local.annotation!.name, "T");
  const later = method.body![1]!;
  assert.ok(later.kind === "binding");
  assert.equal(later.annotation!.name, method.genericParameters[0]);
});
