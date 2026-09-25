import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compile, instantiate, type ReplayEvent } from "../src/compiler.ts";

const PROGRAM = `fn choose(flag: bool, left: i32, right: i32) -> i32:
    if flag:
        left + 1
    else:
        right * 2

fn main() -> i32:
    base := 20
    choose(base < 30, base, 0) * 2
`;

test("checker creates typed HIR with resolved locals and calls", () => {
  const result = analyze(PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.hir?.functions[0]?.result, "i32");
  assert.equal(result.hir?.functions[1]?.locals[0]?.name, "base");
});

test("compiler emits genuine Wasm GC and executes the entry point", async () => {
  const compilation = compile(PROGRAM);
  assert.ok(compilation.wat.includes("type $hd.runtime (struct"));
  assert.ok(WebAssembly.validate(compilation.bytes));
  const { instance } = await instantiate(PROGRAM);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("checked i32 arithmetic traps on overflow", async () => {
  const { instance } = await instantiate("fn main() -> i32: 2147483647 + 1\n");
  assert.throws(() => (instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("the minimum i32 literal forms through unary negation", async () => {
  const { instance } = await instantiate("fn main() -> i32: -2147483648\n");
  assert.equal((instance.exports.main as CallableFunction)(), -2_147_483_648);
  assert.equal(analyze("fn main() -> i32: +2147483648\n").diagnostics[0]?.code, "integer-literal-range");
  const division = await instantiate("fn main() -> i32: -2147483648 / -1\n");
  assert.throws(() => (division.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("integer power is right-associative, checked, and rejects negative exponents", async () => {
  const { instance, compilation } = await instantiate("fn main() -> i32: -2 ** 2 + 2 ** 3 ** 2\n");
  assert.match(compilation.wat, /call \$hd\.pow_i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 508);

  const negative = await instantiate("fn main() -> i32: 2 ** -1\n");
  assert.throws(() => (negative.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
  const overflow = await instantiate("fn main() -> i32: 2 ** 31\n");
  assert.throws(() => (overflow.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("floating power uses the host IEEE pow primitive", async () => {
  const { instance, compilation } = await instantiate("fn main() -> f64: 9.0 ** 0.5\n");
  assert.match(compilation.wat, /import "hd" "pow_f64"/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
  assert.equal(analyze("fn main() -> f64: 2 ** 2.0\n").diagnostics[0]?.code, "mixed-numeric-types");
});

test("checker rejects name, mutability, and type errors", () => {
  assert.equal(analyze("fn main() -> i32: missing\n").diagnostics[0]?.code, "unknown-name");
  assert.equal(analyze("fn main() -> i32:\n    x := 1\n    x = 2\n    x\n").diagnostics[0]?.code, "non-reassignable-binding");
  assert.equal(analyze("fn main() -> i32: true\n").diagnostics[0]?.code, "type-mismatch");
  assert.equal(analyze("fn main() -> f64: 2.0 % 1.0\n").diagnostics[0]?.code, "invalid-binary-operands");
  assert.equal(analyze("fn main() -> bool: true < false\n").diagnostics[0]?.code, "invalid-binary-operands");
  assert.equal(analyze("fn bad(first: i32..., second: i32) -> i32: second\n").diagnostics[0]?.code, "nonfinal-vararg");
  assert.equal(analyze("fn fixed(value: i32) -> i32: value\nfn main(values: list[i32]) -> i32: fixed(values...)\n").diagnostics[0]?.code, "positional-spread-needs-vararg");
});

test("homogeneous varargs lower through the existing list ABI", async () => {
  const source = `fn count(values: i32...) -> i32: values.len()
fn first_or(values: i32...) -> i32:
    if values.len() == 0:
        9
    else:
        values[0]
fn generic_first[T](values: T...) -> T: values[0]
fn tagged(tag: i32, values: i32...) -> i32: tag * 10 + values[0]
fn main() -> i32:
    items := [4, 5, 6]
    count() + count(1, 2, 3) + first_or() + first_or(items...) + generic_first(6) + tagged([2]..., tag=1)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 34);
  const variadic = compilation.hir?.functions.find((fn) => fn.name === "count");
  assert.equal(variadic?.parameters[0]?.type, "list[i32]");
});

test("first-class vararg functions retain their calling convention", async () => {
  const source = `fn count(values: i32...) -> i32: values.len()
fn invoke(callback: fn(i32...) -> i32, items: list[i32]) -> i32:
    callback(1, 2) * 10 + callback(items...)
fn main() -> i32: invoke(count, [3, 4, 5])
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 23);
  assert.equal(analyze(`fn fixed(value: list[i32]) -> i32: value.len()
fn apply_callback(callback: fn(i32...) -> i32) -> i32: callback()
fn main() -> i32: apply_callback(fixed)
`).diagnostics[0]?.code, "type-mismatch");
});

test("function parameter defaults evaluate after explicit arguments", async () => {
  const source = `fn combine(first: i32 = 10, second: i32 = first + 1, third: i32 = second + 1) -> i32:
    first * 100 + second * 10 + third

fn choose[T](value: T, fallback: T = value) -> T: fallback

fn load!(base: i32, extra: i32 = 2) -> i32: base + extra

fn main!() -> i32:
    combine() + combine(third=4, first=2) + choose(40) + load!(40)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 1438);
  assert.ok(compilation.hir?.functions.some((declaration) => declaration.name === "$parameter-default.combine.second"));
  assert.ok(compilation.hir?.functions.some((declaration) => declaration.name === "$parameter-default.choose.fallback"));
});

test("function parameter defaults enforce order, type, and purity", () => {
  assert.equal(analyze("fn bad(first: i32 = 1, second: i32) -> i32: second\n").diagnostics[0]?.code, "parameter-default-order");
  assert.equal(analyze("fn bad(value: i32 = true) -> i32: value\n").diagnostics[0]?.code, "type-mismatch");
  assert.equal(analyze(`fn wait!() -> i32: 1
fn bad(value: i32 = wait!()) -> i32: value
`).diagnostics[0]?.code, "impure-parameter-default");
});

test("varargs work in suspending and trait method calls", async () => {
  const source = `data Box:
    base: i32

trait Sum:
    fn add(self, values: i32...) -> i32
    fn add_async!(self, values: i32...) -> i32

impl Sum for Box:
    fn add(self, values: i32...) -> i32: self.base + values[0] + values[1]
    fn add_async!(self, values: i32...) -> i32: self.base + values[0]

fn use_dynamic(value: Sum) -> i32: value.add(1, 2)
fn use_dynamic_async!(value: Sum) -> i32: value.add_async!(4)

fn main!() -> i32:
    value := Box { base: 30 }
    items := [5, 6]
    value.add(items...) + use_dynamic(value) + use_dynamic_async!(value)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 108);
});

test("named arguments map to parameters without changing source evaluation order", async () => {
  const ordinary = await instantiate(`fn combine(first: i32, second: i32) -> i32: first * 10 + second
fn identity[T](value: T, fallback: T) -> T: value
fn main() -> i32: combine(second=2, first=4) + identity(fallback=0, value=0)
`);
  assert.equal((ordinary.instance.exports.main as CallableFunction)(), 42);

  const source = `fn first!() -> i32: 1
fn second!() -> i32: 2
fn combine(first: i32, second: i32) -> i32: first * 10 + second
fn main!() -> i32:
    combine(second=first!(), first=second!())
`;
  const polls: number[] = [];
  const suspended = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 3) polls.push(functionIndex);
    },
  });
  assert.equal((suspended.instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);
});

test("named enum payloads preserve source evaluation order", async () => {
  const source = `enum Pair[T]:
    Value(first: T, second: T)

fn first!() -> i32: 1
fn second!() -> i32: 2
fn main!() -> i32:
    let pair: Pair[i32] = .Value(second=first!(), first=second!())
    match pair:
        Pair.Value(first, second) => first * 10 + second
`;
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);

  assert.equal(analyze(`enum Pair:
    Value(first: i32, second: i32)
fn main() -> Pair: Pair.Value(missing=1, first=2)
`).diagnostics[0]?.code, "unknown-named-argument");
  assert.equal(analyze(`enum Pair:
    Value(first: i32, second: i32)
fn main() -> Pair: Pair.Value(1, first=2)
`).diagnostics[0]?.code, "duplicate-argument");
});

test("named enum payload patterns resolve bindings by field name", async () => {
  const source = `enum Choice:
    Pair(then_value: i32, else_value: i32)

fn ordered(choice: Choice) -> i32:
    match choice:
        Choice.Pair(else_value=right, then_value=left) => left - right

fn main() -> i32: ordered(Choice.Pair(50, 8))
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze(`enum Choice:
    Pair(left: i32, right: i32)
fn bad(choice: Choice) -> i32:
    match choice:
        Choice.Pair(missing=value, right=other) => value
`).diagnostics[0]?.code, "unknown-variant-pattern-field");
});

test("literal enum payload patterns constrain variants in Wasm and suspension CFG", async () => {
  const source = `data Point:
    x: i32
    y: i32

enum Expr:
    Scale(value: i32, factor: i32)

enum Box[T]:
    Value(value: T)

enum Shape:
    Point(value: Point)

enum Inner:
    Some(value: i32)
    None

enum Outer:
    Wrap(value: Inner)

fn tick!() -> i32: 1

fn evaluate!(expr: Expr) -> i32:
    match expr:
        Expr.Scale(value, factor=2) => tick!() + value * 2
        Expr.Scale(value, factor) => value * factor

fn inspect(box: Box[i32]) -> i32:
    match box:
        Box.Value(42) => 1
        Box.Value(_) => 0

fn inspect_shape!(item: Shape) -> i32:
    match item:
        Shape.Point(Point { x=0, y }) => tick!() + y
        Shape.Point(Point { x, y }) => x + y

fn inspect_nested!(item: Outer) -> i32:
    match item:
        Outer.Wrap(Inner.Some(42)) => tick!() + 41
        Outer.Wrap(Inner.Some(value)) => value
        Outer.Wrap(_) => 0

fn main!() -> i32:
    evaluate!(Expr.Scale(20, 2)) + evaluate!(Expr.Scale(10, 3)) + inspect(Box.Value(42)) + inspect_shape!(Shape.Point(Point { x: 0, y: 9 })) + inspect_shape!(Shape.Point(Point { x: 3, y: 4 })) + inspect_nested!(Outer.Wrap(Inner.Some(42))) + inspect_nested!(Outer.Wrap(Inner.Some(7))) + inspect_nested!(Outer.Wrap(Inner.None))
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 138);
  assert.equal(analyze(`enum Expr:
    Scale(value: i32, factor: i32)
fn incomplete(expr: Expr) -> i32:
    match expr:
        Expr.Scale(value, factor=2) => value
`).diagnostics[0]?.code, "nonexhaustive-match");
});

test("shared enum data uses per-variant factories and pure ordered defaults", async () => {
  const source = `enum Status(code: i32, doubled: i32 = code * 2):
    Unknown -> Status(21)

enum Tagged[T](tag: T):
    Value(value: T) -> Tagged(value)

fn read_tagged(value: Tagged[i32]) -> i32:
    match value:
        Tagged.Value(payload) => value.tag + payload

fn main() -> i32:
    status := Status.Unknown
    status.doubled + read_tagged(Tagged.Value(21))
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 84);
  assert.ok(compilation.hir?.functions.some((declaration) => declaration.name === "$enum-variant.Status.Unknown"));
  assert.ok(compilation.hir?.functions.some((declaration) => declaration.name === "$enum-default.Status.doubled"));
});

test("named arguments work through static and dynamic trait dispatch", async () => {
  const source = `trait Mixer:
    fn blend(self, left: i32, right: i32) -> i32
    fn load!(self, left: i32, right: i32) -> i32

data Calculator: pass

impl Mixer for Calculator:
    fn blend(self, left: i32, right: i32) -> i32: left * 10 + right
    fn load!(self, left: i32, right: i32) -> i32: left * 10 + right

fn main!() -> i32:
    calculator := Calculator {}
    static := calculator.blend(right=2, left=4)
    let dynamic: Mixer = calculator
    direct := dynamic.blend(right=0, left=0)
    suspended := dynamic.load!(right=0, left=0)
    static + direct + suspended
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("prelude names cannot be shadowed by declarations or bindings", () => {
  const cases = [
    "data Display: pass\n",
    "fn println() -> void: pass\n",
    "fn identity[Result](value: Result) -> Result: value\n",
    "fn consume(Console: i32) -> i32: Console\n",
    "fn main() -> i32:\n    let Hash: i32 = 42\n    Hash\n",
  ];
  for (const source of cases) {
    assert.equal(analyze(source).diagnostics[0]?.code, "prelude-name-shadow", source);
  }
});

test("branch scopes do not leak and may shadow each other", () => {
  const source = `fn main() -> i32:
    if true:
        value := 1
        value
    else:
        value := 2
        value
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  assert.equal(analyze("fn main() -> i32:\n    if true:\n        hidden := 1\n    hidden\n").diagnostics[0]?.code, "unknown-name");
});

test("else if chains preserve value typing and selection order", async () => {
  const source = `fn label(score: i32) -> i32:
    if score >= 90:
        40
    else if score >= 70:
        2
    else:
        0

fn main() -> i32: label(95) + label(75)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("while, break, and continue lower to structured Wasm control flow", async () => {
  const source = `fn main() -> i32:
    let value: i32 = 0
    while value < 10:
        value = value + 1
        if value == 3:
            continue
        if value == 7:
            break
    value
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(loop \$loop/);
  assert.equal((instance.exports.main as CallableFunction)(), 7);
  assert.equal(analyze("fn main() -> void: break\n").diagnostics[0]?.code, "break-outside-loop");
});

test("while else produces values on break or normal exhaustion", async () => {
  const broken = `fn main() -> i32:
    let cleanup: i32 = 0
    value := while true:
        defer:
            cleanup = cleanup + 1
        break 41
    else:
        0
    value + cleanup
`;
  const brokenResult = await instantiate(broken);
  assert.equal((brokenResult.instance.exports.main as CallableFunction)(), 42);

  const exhausted = `fn main() -> i32:
    let index: i32 = 0
    value := while index < 2:
        index = index + 1
        continue
    else:
        40
    value + index
`;
  const exhaustedResult = await instantiate(exhausted);
  assert.equal((exhaustedResult.instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn main() -> void:\n    while true:\n        break 1\n").diagnostics[0]?.code, "break-value-context");
  assert.equal(analyze("fn main() -> i32:\n    while true:\n        break\n    else:\n        1\n").diagnostics[0]?.code, "break-value-context");
});

test("for loops iterate lists and maps with continue, destructuring, and else values", async () => {
  const source = `fn load!() -> list[i32]: [1, 2, 3]

fn first_or(values: list[i32]) -> i32:
    for value in values:
        break value
    else:
        1

fn main!() -> i32:
    let total: i32 = 0
    for value in load!():
        if value == 2:
            continue
        total = total + value
    scores := {"a": 10, "b": 20}
    for key, score in scores:
        total = total + score + key.len() - 1
    for entry in {"c": 7}:
        total = total + entry.1
    let cleanup: i32 = 0
    for value in [1, 2]:
        defer:
            cleanup = cleanup + 10
        if value == 1:
            continue
        break
    if total + first_or([]) == 42 and cleanup == 20:
        42
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn bad() -> void:\n    for value in 1:\n        pass\n").diagnostics[0]?.code, "not-iterable");
  assert.equal(analyze("fn bad(values: list[i32]) -> void:\n    for left, right in values:\n        pass\n").diagnostics[0]?.code, "for-binding-arity");
});

test("data declarations lower to Wasm GC structs", async () => {
  const source = `data Point:
    x: i32
    y: i32

fn sum(point: Point) -> i32: point.x + point.y

fn main() -> i32:
    point := Point { y: 22, x: 20 }
    sum(point)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$d0/);
  assert.match(compilation.wat, /\(struct\.get \$d0 \$d0f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable data paths weaken one way and share Wasm GC identity", async () => {
  const source = `data Child:
    name: string

data Parent:
    child: mut Child
    snapshot: Child

fn rename(parent: mut Parent) -> void:
    parent.child.name = "Grace"
    parent.snapshot = Child { name: "updated" }

fn main() -> i32:
    let child: mut Child = Child { name: "Ada" }
    let parent: mut Parent = Parent { child: child, snapshot: Child { name: "old" } }
    readonly := parent
    rename(parent)
    if readonly.child.name == "Grace" and readonly.snapshot.name == "updated":
        42
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /struct\.set \$d0/);
  assert.match(compilation.wat, /field \$d1f0 \(mut/);

  assert.equal(analyze("data User:\n    name: string\nfn bad(user: User) -> void: user.name = \"x\"\n").diagnostics[0]?.code, "readonly-root");
  assert.equal(analyze("data Child:\n    name: string\ndata Parent:\n    child: Child\nfn bad(parent: mut Parent) -> void: parent.child.name = \"x\"\n").diagnostics[0]?.code, "readonly-edge");
});

test("mutable list and map roots support indexed replacement", async () => {
  const source = `fn main() -> i32:
    let values: mut list[i32] = [1, 2]
    let scores: mut map[string, i32] = {"x": 1}
    values[0] = 40
    scores["x"] = 2
    values[0] + match scores["x"]:
        score? => score
        nil => 0
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /array\.set \$hd\.list/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
});

test("mutable lists append through growable Wasm GC storage", async () => {
  const source = `fn main() -> i32:
    let values: mut list[i32] = []
    values.append(1)
    values.append(2)
    values.append(3)
    values.append(4)
    values.append(5)
    values.append(36)
    values.len() + values[5]
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /call \$hd\.vector_append/);
  assert.ok(analyze(`fn bad(values: list[i32]) -> void:
    values.append(1)
`).diagnostics.some((diagnostic) => diagnostic.code === "mutable-receiver-required"));
});

test("mutable maps grow from empty storage and remove entries in insertion order", async () => {
  const source = `fn main() -> i32:
    let values: mut map[string, i32] = {}
    values["a"] = 1
    values["b"] = 2
    values["c"] = 3
    values["d"] = 4
    values["e"] = 5
    values["f"] = 27
    removed := match values.remove(key="c"):
        value? => value
        nil => 0
    missing := match values.get("c"):
        value? => value
        nil => 0
    let remaining = 0
    for key, value in values:
        _ := key
        remaining = remaining + value
    removed + missing + remaining
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /call \$hd\.map_remove/);
  assert.match(compilation.wat, /struct\.set \$hd\.map \$hd\.map-keys/);

  assert.equal(analyze(`fn bad(values: map[string, i32]) -> void:
    _ := values.remove("x")
`).diagnostics[0]?.code, "mutable-receiver-required");
});

test("data field initializers preserve source evaluation order", async () => {
  const source = `data Pair[T]:
    first: T
    second: T

fn first!() -> i32: 1
fn second!() -> i32: 2
fn main!() -> i32:
    pair := Pair { second: first!(), first: second!() }
    pair.first * 10 + pair.second
`;
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);
});

test("data field defaults run per construction after explicit fields", async () => {
  const source = `fn default_name() -> string: "anonymous"

data User:
    id: i32
    name: string = default_name()
    active: bool = true

fn main() -> i32:
    first := User { id: 1 }
    second := User { active: false, name: "Ada", id: 2 }
    if first.name == "anonymous" and first.active and second.name == "Ada" and not second.active:
        first.id * 10 + second.id
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 12);

  const main = compilation.hir?.functions.find((declaration) => declaration.name === "main");
  const firstBinding = main?.body[0];
  assert.equal(firstBinding?.kind, "binding");
  if (firstBinding?.kind === "binding") {
    assert.equal(firstBinding.value.kind, "data");
    if (firstBinding.value.kind === "data") assert.deepEqual(firstBinding.value.fieldIndices, [0, 1, 2]);
  }
});

test("data field defaults are type checked and purity checked", () => {
  assert.equal(analyze(`data Box[T]:
    value: T? = nil
fn make() -> Box[i32]: Box {}
`).diagnostics.length, 0);
  assert.equal(analyze(`fn wait!() -> i32: 1
data Bad:
    value: i32 = wait!()
`).diagnostics[0]?.code, "impure-data-default");
  assert.equal(analyze(`data Bad:
    value: i32 = true
`).diagnostics[0]?.code, "type-mismatch");
});

test("data copy-update evaluates its source before replacements", async () => {
  const source = `data Pair[T]:
    first: T
    second: T

fn source!() -> Pair[i32]: Pair { first: 20, second: 1 }
fn replacement!() -> i32: 22
fn main!() -> i32:
    pair := Pair { ...source!(), second: replacement!() }
    pair.first + pair.second
`;
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(polls, [0, 1]);
});

test("data copy-update supplies omitted fields without running defaults", async () => {
  const source = `data Counter:
    value: i32 = panic("copy-update evaluated a default")

fn main() -> i32:
    original := Counter { value: 42 }
    copied := Counter { ...original }
    copied.value
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze(`data Other:
    value: i32
data Counter:
    value: i32
fn bad(other: Other) -> Counter: Counter { ...other }
`).diagnostics[0]?.code, "type-mismatch");
});

test("fieldless data lowers to an empty Wasm GC struct", async () => {
  const source = `data Unit: pass

fn consume(value: Unit) -> i32: 42
fn main() -> i32: consume(Unit {})
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct\s*\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("data initialization checks field names, presence, and types", () => {
  const header = "data Point:\n    x: i32\n";
  assert.equal(analyze(`${header}fn main() -> i32: Point { y: 1 }.x\n`).diagnostics[0]?.code, "unknown-data-field");
  assert.equal(analyze(`${header}fn main() -> i32: Point { }.x\n`).diagnostics[0]?.code, "missing-required-field");
  assert.equal(analyze(`${header}fn main() -> i32: Point { x: true }.x\n`).diagnostics[0]?.code, "type-mismatch");
});

test("data patterns destructure nested fields and test literals", async () => {
  const source = `data User:
    age: i32
    active: bool

data Envelope:
    user: User

fn label(user: User) -> i32:
    match user:
        User { age, active=true } => age
        User { age=alias } => alias

fn nested(envelope: Envelope) -> i32:
    match envelope:
        Envelope { user=User { age } } => age

fn main() -> i32:
    label(User { age: 40, active: true }) + label(User { age: 1, active: false }) + nested(Envelope { user: User { age: 1, active: false } })
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /struct\.get \$d0 \$d0f0 \(struct\.get \$d1 \$d1f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const header = "data User:\n    age: i32\n    active: bool\n\n";
  assert.equal(analyze(`${header}fn read(user: User) -> i32:\n    match user:\n        User { age, age=other } => age\n`).diagnostics[0]?.code, "duplicate-data-pattern-field");
  assert.equal(analyze(`${header}fn read(user: User) -> i32:\n    match user:\n        User { missing } => 1\n`).diagnostics[0]?.code, "unknown-data-field");
  assert.equal(analyze(`${header}fn read(user: User) -> i32:\n    match user:\n        User { age=1 } => 1\n`).diagnostics[0]?.code, "nonexhaustive-match");
});

test("strings use GC byte arrays and len counts Unicode scalars", async () => {
  const source = `data User:
    name: string

fn name_length(user: User) -> i32: user.name.len()

fn main() -> i32: name_length(User { name: "A界😀" })
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.bytes 8/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
});

test("string interpolation displays built-ins from left to right", async () => {
  const source = `fn main() -> i32:
    text := "\${-2147483648}|\${0}|\${42}|\${true}|\${false}|\${'😀'}"
    if text == "-2147483648|0|42|true|false|😀":
        text.len()
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 29);
  assert.match(compilation.wat, /call \$hd\.i32_to_string/);
  assert.match(compilation.wat, /call \$hd\.char_to_string/);
  assert.equal(analyze('fn main() -> string: "${1.5}"\n').diagnostics[0]?.code, "missing-display");
});

test("strings compare by UTF-8 value order", async () => {
  const source = `fn main() -> i32:
    if "same" == "same" and "a" < "é" and "é" < "界" and "abc" < "abcd" and "界面".starts_with("界") and "abc".starts_with("") and not "a".starts_with("ab"):
        42
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /call \$hd\.string_starts_with/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("strings concatenate and unnamed enum fields use numeric selectors", async () => {
  const source = `enum StatusCode(i32):
    Answer -> StatusCode(40)

fn main() -> i32:
    status := StatusCode.Answer
    message := "A" + "界"
    status.0 + message.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /call \$hd\.string_concat/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("character literals carry Unicode scalar values and compare in scalar order", async () => {
  const source = `fn main() -> char:
    if 'A' < '界' and '😀' == '\\u{1F600}':
        '😀'
    else:
        'x'
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 0x1f600);
});

test("defer runs after a return value is evaluated", async () => {
  const source = `fn main() -> i32:
    let value: i32 = 1
    defer:
        value = value + 10
    return value
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /local\.set \$tmp0/);
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});

test("defer rejects escaping control flow", () => {
  const source = "fn main() -> i32:\n    defer:\n        return 1\n    0\n";
  assert.equal(analyze(source).diagnostics[0]?.code, "defer-control-flow");
});

test("defer is LIFO and runs on loop continue and break", async () => {
  const source = `fn main() -> i32:
    let value: i32 = 0
    if true:
        defer:
            value = value * 10 + 1
        defer:
            value = value * 10 + 2
        pass
    let iterations: i32 = 0
    while iterations < 2:
        defer:
            value = value + 100
        iterations = iterations + 1
        continue
    while true:
        defer:
            value = value + 1000
        break
    value
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 1221);
});

test("explicit panic lowers to unreachable and skips pending defer", async () => {
  const source = `fn main() -> i32:
    defer:
        panic("cleanup must not run")
    panic("boom")
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /unreachable/);
  assert.throws(() => (instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("enums use tagged GC structs and match binds payloads", async () => {
  const source = `enum Outcome:
    Value(value: i32)
    Failure(code: i32)
    Empty

fn read(outcome: Outcome) -> i32:
    match outcome:
        Outcome.Value(value) => value
        Outcome.Failure(code) => -code
        Outcome.Empty => 0

fn main() -> i32: read(Outcome.Value(42))
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$e0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$e0 \(i32\.const 0\)/);
  assert.match(compilation.wat, /\(struct\.get \$e0 \$e0tag/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("match checking enforces coverage, payload arity, and arm reachability", () => {
  const header = "enum Flag:\n    On(value: i32)\n    Off\n\n";
  assert.equal(analyze(`${header}fn main(flag: Flag) -> i32:\n    match flag:\n        Flag.On(value) => value\n`).diagnostics[0]?.code, "nonexhaustive-match");
  assert.equal(analyze(`${header}fn main(flag: Flag) -> i32:\n    match flag:\n        Flag.On => 1\n        Flag.Off => 0\n`).diagnostics[0]?.code, "pattern-arity");
  assert.equal(analyze(`${header}fn main(flag: Flag) -> i32:\n    match flag:\n        _ => 1\n        Flag.Off => 0\n`).diagnostics[0]?.code, "unreachable-match-arm");
  assert.equal(analyze(`${header}fn main(flag: Flag) -> i32:\n    match flag:\n        On => 1\n        .Off => 0\n`).diagnostics[0]?.code, "bare-variant-pattern");
});

test("contextual enum variant patterns use the subject type", async () => {
  const source = `enum Flag:
    On(value: i32)
    Off

fn read(flag: Flag) -> i32:
    match flag:
        .On(value) => value
        .Off => 0

fn main() -> i32: read(Flag.On(42))
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const swapped = analyze(`enum Pair:
    Values(left: i32, right: i32)
fn read(value: Pair) -> i32:
    match value:
        Pair.Values(right, left) => left - right
`);
  assert.ok(swapped.hir);
  assert.ok(swapped.diagnostics.some((diagnostic) => diagnostic.code === "variant-binding-name-mismatch" && diagnostic.severity === "warning"));
});

test("contextual enum variant expressions use their expected type", async () => {
  const source = `enum Status:
    Queued
    Ready(value: i32)

fn queued() -> Status: .Queued
fn unwrap(status: Status) -> i32:
    match status:
        .Queued => 0
        .Ready(value) => value

fn accept(status: Status) -> i32: unwrap(status)
fn main() -> i32: unwrap(queued()) + accept(.Ready(42))
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("enum Status:\n    Queued\nfn main() -> void:\n    status := .Queued\n").diagnostics[0]?.code, "missing-contextual-enum-type");
});

test("boolean matches are exhaustive and lower to scalar tests", async () => {
  const source = `fn choose(flag: bool) -> i32:
    match flag:
        true => 42
        false => 0

fn main() -> i32: choose(true) + choose(false)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /i32\.eq[\s\S]*local\.get \$tmp/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn choose(flag: bool) -> i32:\n    match flag:\n        true => 1\n").diagnostics[0]?.code, "nonexhaustive-match");
  assert.equal(analyze("fn choose(flag: bool) -> i32:\n    match flag:\n        true => 1\n        true => 2\n        false => 0\n").diagnostics[0]?.code, "unreachable-match-arm");
});

test("numeric, character, and string literal patterns require a catch-all", async () => {
  const source = `fn integer(value: i32) -> i32:
    match value:
        -1 => 10
        42 => 20
        other => other

fn floating(value: f64) -> i32:
    match value:
        -1.5 => 3
        _ => 0

fn character(value: char) -> i32:
    match value:
        '😀' => 4
        _ => 0

fn text(value: string) -> i32:
    match value:
        "hd" => 5
        _ => 0

fn main() -> i32: integer(42) + floating(-1.5) + character('😀') + text("hd") + integer(0)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /f64\.eq/);
  assert.equal((instance.exports.main as CallableFunction)(), 32);
  assert.equal(analyze("fn classify(value: i32) -> i32:\n    match value:\n        1 => 1\n").diagnostics[0]?.code, "nonexhaustive-match");
  assert.equal(analyze("fn classify(value: char) -> i32:\n    match value:\n        'x' => 1\n        'x' => 2\n        _ => 0\n").diagnostics[0]?.code, "unreachable-match-arm");
});

test("match guards see pattern bindings and do not contribute coverage", async () => {
  const source = `enum Number:
    Value(value: i32)
    Empty

fn classify(number: Number) -> i32:
    match number:
        Number.Value(value) if value > 10 => 40
        Number.Value(value) => value
        Number.Empty => 0

fn sign(value: i32) -> i32:
    match value:
        candidate if candidate < 0 => -1
        _ => 1

fn main() -> i32: classify(Number.Value(2)) + classify(Number.Value(12)) + sign(-1) + sign(0)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn classify(value: bool) -> i32:\n    match value:\n        _ if true => 1\n").diagnostics[0]?.code, "nonexhaustive-match");
  assert.equal(analyze("fn classify(value: bool) -> i32:\n    match value:\n        candidate if candidate => 1\n        _ => 0\n").diagnostics.length, 0);
  assert.equal(analyze("fn classify(value: bool) -> i32:\n    match value:\n        _ if 1 => 1\n        _ => 0\n").diagnostics[0]?.code, "type-mismatch");
});

test("optionals inject plain values, match both cases, and propagate nil", async () => {
  const source = `fn maybe(flag: bool) -> i32?:
    if flag:
        40
    else:
        nil

fn lifted(flag: bool) -> i32?:
    value := maybe(flag)?
    value + 2

fn inspect(value: i32?) -> i32:
    match value:
        nil => -1
        actual? => actual

fn main() -> i32: inspect(lifted(true)) + inspect(lifted(false))
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.variant/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 41);
});

test("Result constructors, matching, and error propagation use the erased carrier", async () => {
  const source = `data ParseError:
    code: i32

fn parse(ok: bool) -> Result[i32, ParseError]:
    if ok:
        Ok(40)
    else:
        Err(ParseError { code: 7 })

fn lifted(ok: bool) -> Result[i32, ParseError]:
    value := parse(ok)?
    Ok(value + 2)

fn inspect(value: Result[i32, ParseError]) -> i32:
    match value:
        Ok(actual) => actual
        Err(error) => -error.code

fn main() -> i32: inspect(lifted(true)) + inspect(lifted(false))
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 35);
});

test("imported ResourceError is a generic Wasm GC enum", async () => {
  const source = `use std.resource.ResourceError

data Failure:
    code: i32

fn make() -> ResourceError[Failure]:
    ResourceError.Operation(Failure { code: 42 })

fn main() -> i32:
    match make():
        ResourceError.Operation(error) => error.code
        ResourceError.Disposed => 0
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.enums.at(-1)?.name, "ResourceError");
  assert.match(compilation.wat, /type \$e\d+ \(struct/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("Result patterns recursively match imported enum payloads", async () => {
  const source = `use std.resource.ResourceError

data Failure:
    code: i32

fn outcome() -> Result[i32, ResourceError[Failure]]:
    Err(ResourceError.Disposed)

fn main() -> i32:
    match outcome():
        Err(ResourceError.Disposed) => 42
        _ => 0
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("optional and Result context errors have stable diagnostics", () => {
  assert.equal(analyze("value := nil\n").diagnostics[0]?.code, "nil-needs-optional-type");
  assert.equal(analyze("value := Ok(1)\n").diagnostics[0]?.code, "result-constructor-needs-context");
  assert.equal(analyze("fn main() -> i32:\n    value := 1\n    value?\n").diagnostics[0]?.code, "invalid-result-propagation");
  const nested = analyze("let nested: string?? = nil\n");
  assert.ok(nested.hir);
  assert.equal(nested.diagnostics[0]?.code, "unused-local-binding");
});

test("optional and Result values are must-use unless explicitly discarded", () => {
  const prefix = "fn maybe() -> i32?: nil\n";
  assert.equal(analyze(`${prefix}fn main() -> void: maybe()\n`).diagnostics[0]?.code, "discarded-must-use-value");
  assert.deepEqual(analyze(`${prefix}fn main() -> void: _ := maybe()\n`).diagnostics, []);
});

test("typed noncapturing closures lower to Wasm typed function references", async () => {
  const source = `fn apply(value: i32, transform: fn(i32) -> i32) -> i32:
    transform(value)

fn main() -> i32:
    increment := fn(value: i32) -> i32: value + 1
    apply(41, increment)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$sig0 \(func/);
  assert.match(compilation.wat, /call_ref \$sig0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn main() -> i32:\n    apply := fn(value: i32) -> i32: value\n    apply(true)\n").diagnostics[0]?.code, "type-mismatch");
});

test("expected function types infer inline closure parameters and results", async () => {
  const source = `fn apply(value: i32, transform: fn(i32) -> i32) -> i32:
    transform(value)

fn main() -> i32:
    let increment: fn(i32) -> i32 = fn(value): value + 1
    apply(40, increment) + apply(0, fn(value): value + 1)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn main() -> i32:\n    closure := fn(value) -> i32: value\n    0\n").diagnostics[0]?.code, "closure-parameter-needs-annotation");
});

test("nonrecursive closures infer result types from fallthrough and returns", async () => {
  const source = `fn main() -> i32:
    identity := fn(value: i32): value
    choose := fn(value: i32):
        if value > 0:
            return value
        0
    identity(40) + choose(2)
`;
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn(i32)->i32");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn main() -> i32:\n    bad := fn(value: i32):\n        if value > 0:\n            return 1\n        return true\n    0\n").diagnostics[0]?.code, "closure-result-type");
});

test("explicitly typed local closures recurse through their current environment", async () => {
  const source = `fn main() -> i32:
    sum := fn(value: i32) -> i32:
        if value == 0:
            0
        else:
            value + sum(value - 1)
    sum(9) - 3
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /ref\.func \$c0\) \(local\.get \$env\)/);
  assert.equal(compilation.hir.closures[0]?.captures.length, 0);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("named local functions capture enclosing values and recurse", async () => {
  const source = `fn total_with_bonus(values: list[i32], bonus: i32) -> i32:
    fn add_bonus(value: i32) -> i32:
        value + bonus
    let total: i32 = 0
    for value in values:
        total = total + add_bonus(value)
    total

fn factorial(n: i32) -> i32:
    fn step(value: i32) -> i32:
        if value <= 1:
            1
        else:
            value * step(value - 1)
    step(n)

fn main() -> i32: total_with_bonus([1, 2, 3], 10) + factorial(3)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("trailing callback blocks lower as contextually typed closures", async () => {
  const source = `fn apply(callback: fn() -> i32) -> i32: callback()
fn add(base: i32, callback: fn() -> i32) -> i32: base + callback()
fn main() -> i32:
    left := apply:
        40
    add(left):
        2
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("readonly data fields can call closures that return mutable access", async () => {
  const source = `data Child:
    value: i32
data Holder:
    get: fn() -> mut Child
fn make() -> mut Child: Child { value: 42 }
fn main() -> i32:
    holder := Holder { get: make }
    let child: mut Child = holder.get()
    child.value
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("named functions reify as monomorphic function values", async () => {
  const source = `fn increment(value: i32) -> i32: value + 1

fn apply(value: i32, transform: fn(i32) -> i32) -> i32:
    transform(value)

fn main() -> i32: apply(41, increment)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$fv0 \(type \$sig/);
  assert.match(compilation.wat, /ref\.func \$fv0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("erased generic functions infer type arguments and box primitive values", async () => {
  const source = `data Box:
    value: i32

fn identity[T](value: T) -> T: value

fn choose[T](left: T, right: T) -> T: left

fn float_value() -> f64: identity(3.5)

fn main() -> i32:
    number := identity(40)
    text := identity("hd")
    box := identity(Box { value: 2 })
    choose(number + box.value, 42) + text.len() - 2
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$l0 anyref/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-f64\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal((instance.exports.float_value as CallableFunction)(), 3.5);

  assert.equal(analyze("fn choose[T](left: T, right: T) -> T: left\nfn main() -> i32: choose(1, true)\n").diagnostics[0]?.code, "generic-type-mismatch");
  assert.equal(analyze("fn missing[T]() -> i32: 1\nfn main() -> i32: missing()\n").diagnostics[0]?.code, "unresolved-generic-placeholder");
  assert.equal(analyze("fn identity[T](value: T) -> T: value\nfn main() -> void:\n    value := identity\n").diagnostics[0]?.code, "generic-function-value-needs-arguments");
});

test("generic inference traverses optional and Result types", async () => {
  const source = `data Failure:
    code: i32

fn keep_optional[T](value: T?) -> T?: value

fn keep_result[T, E](value: Result[T, E]) -> Result[T, E]: value

fn inspect_optional(value: i32?) -> i32:
    match value:
        actual? => actual
        nil => 0

fn inspect_result(value: Result[i32, Failure]) -> i32:
    match value:
        Ok(actual) => actual
        Err(error) => -error.code

fn main() -> i32:
    let optional: i32? = 40
    let result: Result[i32, Failure] = Ok(2)
    inspect_optional(keep_optional(optional)) + inspect_result(keep_result(result))
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("higher-order erased generics adapt concrete callable ABIs", async () => {
  const source = `fn apply[T](value: T, transform: fn(T) -> T) -> T:
    transform(value)

fn increment(value: i32) -> i32: value + 1

fn main() -> i32:
    apply(40, increment) + apply(0, fn(value): value + 1)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$adapt0/);
  assert.match(compilation.wat, /ref\.cast \(ref \$closure/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic requirement rows infer callback providers and pack them for Wasm GC", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Logger:
    invoke(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.hir.functions[0]?.rowParameters, ["r"]);
  assert.equal(compilation.hir.functions[0]?.parameters[0]?.type, "fn()->i32$row:r");
  assert.match(compilation.wat, /type \$hd\.providers \(struct/);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.match(compilation.wat, /call \$hd\.provider_get/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 42);
});

test("generic requirement rows infer empty rows and diagnose unavailable or conflicting rows", async () => {
  const empty = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r: callback()
fn pure() -> i32: 42
fn main() -> i32: invoke(pure)
`;
  const { instance, compilation } = await instantiate(empty);
  assert.match(compilation.wat, /ref\.null \$hd\.providers/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const missing = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r: callback()
fn read() -> i32 $ Clock + Logger: 42
fn main() -> i32 $ Clock: invoke(read)
`;
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-requirement");

  const conflicting = `fn combine[r](left: fn() -> i32 $ r, right: fn() -> i32 $ r) -> i32 $ r:
    left() + right()
fn clock() -> i32 $ Clock: 20
fn logger() -> i32 $ Logger: 22
fn main() -> i32 $ Clock + Logger: combine(clock, logger)
`;
  assert.equal(analyze(conflicting).diagnostics[0]?.code, "generic-type-mismatch");
});

test("generic row subtraction restores a locally supplied provider", async () => {
  const source = `fn provide_logger[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger) + Backup:
    $.with(Logger=$.use(Backup)):
        callback()

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Backup:
    provide_logger(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.hir.functions[0]?.requirements, ["Backup", "row:r\\Logger"]);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);

  const unsound = `fn drop_logger[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger):
    callback()
fn read() -> i32 $ Logger: 42
fn main() -> i32: drop_logger(read)
`;
  assert.equal(analyze(unsound).diagnostics[0]?.code, "missing-requirement");

  const redundant = `fn redundant[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger) + Backup:
    $.with(Logger=$.use(Backup)):
        callback()
fn tick() -> i32 $ Clock: 42
fn main() -> i32 $ Clock + Backup: redundant(tick)
`;
  const warning = analyze(redundant);
  assert.ok(warning.hir);
  assert.deepEqual(warning.diagnostics.map(({ code, severity }) => ({ code, severity })), [
    { code: "requirement-subtract-absent", severity: "warning" },
  ]);
});

test("generic row union inference chooses the least row solution", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r + Logger) -> i32 $ r + Logger:
    callback()
fn logged() -> i32 $ Logger: 42
fn main() -> i32 $ Logger: invoke(logged)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)({ logger: true }), 42);
});

test("generic row forwarding composes symbolic and concrete provider packs", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn forward[s](callback: fn() -> i32 $ s + Clock) -> i32 $ s + Clock:
    invoke(callback)

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Logger:
    forward(read)
`;
  const { instance, compilation } = await instantiate(source);
  const forwarded = compilation.hir.functions[1]?.body[0];
  assert.equal(forwarded?.kind, "expression");
  assert.equal(forwarded?.kind === "expression" && forwarded.expression.kind, "call");
  const pack = forwarded?.kind === "expression" && forwarded.expression.kind === "call"
    ? forwarded.expression.providers[0]
    : undefined;
  assert.equal(pack?.kind, "provider-pack");
  assert.deepEqual(pack?.kind === "provider-pack" && pack.keys, ["Clock"]);
  assert.equal(pack?.kind === "provider-pack" && pack.bases.length, 1);
  assert.match(compilation.wat, /call \$hd\.provider_concat/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 42);
});

test("generic row forwarding unions multiple symbolic provider packs", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn forward[s, t](left: fn() -> i32 $ s, right: fn() -> i32 $ t, callback: fn() -> i32 $ s + t) -> i32 $ s + t:
    a := left()
    b := right()
    a + b + invoke(callback)

fn tick() -> i32 $ Clock: 1
fn log() -> i32 $ Logger: 2
fn read() -> i32 $ Clock + Logger: 42

fn main() -> i32 $ Clock + Logger:
    forward(tick, log, read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$hd\.provider_concat/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 45);
});

test("generic provider keys reject possible collisions before erasure", async () => {
  const collision = `trait Repo[T]
fn choose[T, U](first: Repo[T], second: Repo[U]) -> void:
    $.with(Repo[T]=first):
        $.with(Repo[U]=second):
            pass
`;
  assert.equal(analyze(collision).diagnostics[0]?.code, "generic-requirement-key-collision");

  const spreadCollision = `trait Repo[T]
fn choose[T, U](first: Repo[T], second: Repo[U]) -> void:
    base := $.context(Repo[T]=first)
    _ := $.context(...base, Repo[U]=second)
`;
  assert.equal(analyze(spreadCollision).diagnostics[0]?.code, "generic-requirement-key-collision");

  const consistentSubstitution = `trait Pair[A, B]
fn safe[T](same: Pair[T, T], mixed: Pair[i32, string]) -> void:
    $.with(Pair[T, T]=same, Pair[i32, string]=mixed):
        pass
`;
  assert.deepEqual(analyze(consistentSubstitution).diagnostics, []);
  const occursCheck = `trait Repo[T]
fn safe[T](plain: Repo[T], nested: Repo[T?]) -> void:
    $.with(Repo[T]=plain, Repo[T?]=nested):
        pass
`;
  assert.deepEqual(analyze(occursCheck).diagnostics, []);

  const distinct = `trait Repo[T]
data User: pass
data Post: pass
fn choose(first: Repo[User], second: Repo[Post]) -> void:
    $.with(Repo[User]=first, Repo[Post]=second):
        pass
fn consume[T](repo: Repo[T]) -> i32 $ Repo[T]:
    _ := $.use(Repo[T])
    1
fn run(repo: Repo[User]) -> i32:
    $.with(Repo[User]=repo):
        consume(repo)
`;
  const compilation = compile(distinct);
  assert.deepEqual(compilation.diagnostics, []);
  assert.ok(WebAssembly.validate(compilation.bytes));
});

test("trait implementations support static and Wasm GC dynamic dispatch", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data User:
    value: i32

impl Describe for User:
    fn describe(self) -> i32: self.value

fn dynamic_show(value: Describe) -> i32:
    value.describe()

fn main() -> i32:
    user := User { value: 21 }
    user.describe() + dynamic_show(user)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$trait0 \(struct/);
  assert.match(compilation.wat, /func \$tadapt0_0/);
  assert.match(compilation.wat, /call_ref \$tsig0_0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable trait receivers preserve permission through every dispatch path", async () => {
  const source = `trait Add:
    fn add(mut self, value: i32) -> void
    fn twice(mut self, value: i32) -> void:
        self.add(value)
        self.add(value)

data Counter:
    value: i32

impl Add for Counter:
    fn add(mut self, value: i32) -> void:
        self.value = self.value + value

fn bounded[T: Add](value: mut T, amount: i32) -> void:
    value.add(amount)

fn main() -> i32:
    let counter: mut Counter = Counter { value: 0 }
    counter.add(10)
    let dynamic: mut Add = counter
    dynamic.twice(5)
    bounded(counter, 22)
    counter.value
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.traits[0]?.methods[0]?.receiverMutable, true);
  assert.equal(compilation.hir.traits[0]?.methods[1]?.receiverMutable, true);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("inherent methods lower as direct functions with mutable and suspending receivers", async () => {
  const source = `data Counter:
    value: i32

impl Counter:
    fn add(mut self, amount: i32) -> void:
        self.value = self.value + amount
    fn load!(self, amount: i32) -> i32:
        self.value + amount

fn main!() -> i32:
    let counter: mut Counter = Counter { value: 10 }
    counter.add(amount=12)
    counter.load!(20)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$f\d+/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("embedded fields promote inherent methods and can fill readonly trait requirements", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data Label:
    value: i32

impl Label:
    fn describe(self) -> i32: self.value

data Page:
    Label

impl Describe for Page

fn show(value: Describe) -> i32: value.describe()

fn main() -> i32:
    page := Page { Label: Label { value: 21 } }
    page.describe() + show(page)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable trait receivers reject readonly calls and signature weakening", () => {
  const readonlyStatic = `trait Add:
    fn add(mut self) -> void
data Counter:
    value: i32
impl Add for Counter:
    fn add(mut self) -> void: pass
fn invalid(value: Counter) -> void: value.add()
`;
  assert.equal(analyze(readonlyStatic).diagnostics[0]?.code, "mutable-receiver-required");

  const readonlyDynamic = readonlyStatic.replace(
    "fn invalid(value: Counter) -> void: value.add()",
    "fn invalid(value: Add) -> void: value.add()",
  );
  assert.equal(analyze(readonlyDynamic).diagnostics[0]?.code, "mutable-receiver-required");

  const mismatched = readonlyStatic
    .replace("fn add(mut self) -> void: pass", "fn add(self) -> void: pass")
    .replace("fn invalid(value: Counter) -> void: value.add()\n", "");
  assert.equal(analyze(mismatched).diagnostics[0]?.code, "trait-method-signature");
});

test("trait checking diagnoses missing, mismatched, and ambiguous methods", () => {
  const missing = `trait Named:
    fn name(self) -> string
data User:
    value: i32
impl Named for User
`;
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-trait-method");

  const mismatched = `trait Named:
    fn name(self) -> string
data User:
    value: i32
impl Named for User:
    fn name(self) -> i32: 0
`;
  assert.equal(analyze(mismatched).diagnostics[0]?.code, "trait-method-signature");

  const ambiguous = `trait Left:
    fn label(self) -> i32
trait Right:
    fn label(self) -> i32
data User:
    value: i32
impl Left for User:
    fn label(self) -> i32: self.value
impl Right for User:
    fn label(self) -> i32: self.value
fn main() -> i32: User { value: 42 }.label()
`;
  assert.equal(analyze(ambiguous).diagnostics[0]?.code, "ambiguous-method");
});

test("default trait methods participate in static and dynamic dispatch", async () => {
  const source = `trait Named:
    fn name(self) -> i32
    fn label(self) -> i32:
        self.name() + 2

data Number:
    value: i32

impl Named for Number:
    fn name(self) -> i32: self.value

fn show(value: Named) -> i32:
    value.label()

fn main() -> i32:
    show(Number { value: 40 })
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const override = source.replace("fn name(self) -> i32: self.value", "fn name(self) -> i32: self.value\n    fn label(self) -> i32: 41");
  const overridden = await instantiate(override);
  assert.equal((overridden.instance.exports.main as CallableFunction)(), 41);
});

test("suspending trait methods use concrete and dynamic Wasm GC frames", async () => {
  const source = `trait Read:
    fn read!(self, delta: i32) -> i32

data Counter:
    value: i32

impl Read for Counter:
    fn read!(self, delta: i32) -> i32: self.value + delta

fn bounded![T: Read](value: T) -> i32:
    value.read!(1)

fn main!() -> i32:
    value := Counter { value: 10 }
    static := value.read!(0)
    let dynamic: Read = value
    let pending: mut Suspend[i32] = dynamic.read(1)
    stored := pending!()
    direct := dynamic.read!(0)
    generic := bounded!(value)
    static + stored + direct + generic
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 2 && pollCount === 1,
  });
  assert.match(compilation.wat, /type \$ts0_0 \(struct/);
  assert.match(compilation.wat, /ref\.func \$tspolladapt0_0/);
  assert.match(compilation.wat, /call_ref \$tspollsig0_0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("cancelling a dynamic suspending trait method reaches child cleanup", async () => {
  const source = `trait Job:
    fn run!(self) -> void

data Worker:
    value: i32

fn wait!() -> void: pass

impl Job for Worker:
    fn run!(self) -> void:
        defer:
            _ := self.value
        wait!()

fn main!() -> void:
    let job: Job = Worker { value: 42 }
    job.run!()
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  (instance.exports.__hd_start as CallableFunction)();
  assert.equal((instance.exports.__hd_poll as CallableFunction)(), 0);
  (instance.exports.__hd_cancel as CallableFunction)();
  assert.ok(events.some(([functionIndex, event]) => functionIndex === 2 && event === 7));
});

test("default suspending trait methods lower for each implementation", async () => {
  const source = `trait Named:
    fn value(self) -> i32
    fn load!(self) -> i32:
        self.value()

data Number:
    value: i32

impl Named for Number:
    fn value(self) -> i32: self.value

fn main!() -> i32:
    let named: Named = Number { value: 42 }
    named.load!()
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("trait values act as lexical providers and survive generic row subtraction", async () => {
  const source = `trait Logger:
    fn adjust(self, value: i32) -> i32

data OffsetLogger:
    offset: i32

impl Logger for OffsetLogger:
    fn adjust(self, value: i32) -> i32: value + self.offset

fn read() -> i32 $ Logger:
    $.use(Logger).adjust(40)

fn provide_logger[r](callback: fn() -> i32 $ r, logger: Logger) -> i32 $ (r - Logger):
    $.with(Logger=logger):
        callback()

fn main() -> i32:
    provide_logger(read, OffsetLogger { offset: 2 })
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /field \$hd\.provider-value anyref/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$trait0\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("erased generic trait bounds dispatch methods and unwrap returned values", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data Number:
    value: i32

impl Describe for Number:
    fn describe(self) -> i32: self.value

fn show[T: Describe](value: T) -> i32:
    value.describe()

fn identity[T: Describe](value: T) -> T:
    value

fn main() -> i32:
    returned := identity(Number { value: 22 })
    show(Number { value: 20 }) + returned.value
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$bound0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.new \$trait0 \(ref\.null any\)/);
  assert.match(compilation.wat, /struct\.get \$trait0 \$trait0value/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const missing = source.replace("show(Number { value: 20 })", "show(42)");
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-trait-implementation");
});

test("multiple trait bounds pass independent dictionaries and forward them", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

trait Named:
    fn name(self) -> i32

data Number:
    value: i32
    extra: i32

impl Describe for Number:
    fn describe(self) -> i32: self.value

impl Named for Number:
    fn name(self) -> i32: self.extra

fn describe_one[T: Describe](value: T) -> i32:
    value.describe()

fn inspect[T: Describe + Named](value: T) -> i32:
    describe_one(value) + value.name()

fn inspect_first[T: Describe](values: list[T]) -> i32:
    values[0].describe()

fn identity[T: Describe + Named](value: T) -> T:
    value

fn main() -> i32:
    number := identity(Number { value: 20, extra: 2 })
    inspect(number) + inspect_first([Number { value: 20, extra: 0 }])
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$bound0 \(ref null \$trait0\).*param \$bound1 \(ref null \$trait1\)/s);
  assert.match(compilation.wat, /local\.get \$bound0/);
  assert.match(compilation.wat, /local\.get \$bound1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const duplicate = source.replace("T: Describe + Named", "T: Describe + Describe");
  assert.equal(analyze(duplicate).diagnostics[0]?.code, "duplicate-trait-bound");
});

test("generic data uses one erased GC layout with precise instantiated member types", async () => {
  const source = `data Box[T]:
    value: T

data Pair[A, B]:
    first: A
    second: B

fn keep[T](box: Box[T]) -> Box[T]: box

fn main() -> i32:
    box := Box { value: 40 }
    pair := Pair { first: keep(box), second: "hd" }
    pair.first.value + pair.second.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.functions.at(-1)?.locals[0]?.type, "Box[i32]");
  assert.match(compilation.wat, /field \$d0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$d0\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const mismatch = `data Box[T]:\n    value: T\nfn main() -> void:\n    let box: Box[i32] = Box { value: true }\n`;
  assert.equal(analyze(mismatch).diagnostics[0]?.code, "generic-type-mismatch");
});

test("generic enums erase exact payloads and recover instantiated match bindings", async () => {
  const source = `enum Maybe[T]:
    Some(value: T)
    None

enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])

fn keep[T](value: Maybe[T]) -> Maybe[T]: value

fn unwrap(value: Maybe[i32]) -> i32:
    match value:
        Maybe.Some(actual) => actual
        Maybe.None => 0

fn sum(tree: Tree[i32]) -> i32:
    match tree:
        Tree.Leaf(value) => value
        Tree.Branch(left, right) => sum(left) + sum(right)

fn main() -> i32:
    some := Maybe.Some(40)
    let none: Maybe[i32] = Maybe.None
    tree := Tree.Branch(Tree.Leaf(1), Tree.Leaf(1))
    unwrap(keep(some)) + unwrap(none) + sum(tree)
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.functions.at(-1)?.locals[0]?.type, "Maybe[i32]");
  assert.match(compilation.wat, /field \$e0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(analyze("enum Maybe[T]:\n    Some(value: T)\n    None\nfn main() -> void:\n    value := Maybe.None\n").diagnostics[0]?.code, "generic-enum-needs-context");
  assert.equal(analyze("enum Maybe[T]:\n    Some(value: T)\n    None\nfn consume(value: Maybe) -> void:\n    pass\n").diagnostics[0]?.code, "unknown-type");
});

test("generic lists lower to growable GC vectors with erased element storage", async () => {
  const source = `fn first[T](items: list[T]) -> T:
    items[0]

fn main() -> i32:
    numbers := [40, 2]
    first(numbers) + numbers.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /type \$hd\.list \(array \(mut anyref\)\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.vector/);
  assert.match(compilation.wat, /call \$hd\.vector_get/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(analyze("fn main() -> void:\n    values := []\n").diagnostics[0]?.code, "empty-list-needs-context");
  assert.equal(analyze("fn main() -> void:\n    values := [1, true]\n").diagnostics[0]?.code, "no-common-type");
  assert.equal(analyze("fn consume(values: list[void]) -> void:\n    pass\n").diagnostics[0]?.code, "unknown-type");
});

test("contextual list elements convert to dynamic trait values", async () => {
  const source = `trait Value:
    fn get(self) -> i32

data Number:
    value: i32

impl Value for Number:
    fn get(self) -> i32: self.value

fn main() -> i32:
    let values: list[Value] = [Number { value: 20 }, Number { value: 22 }]
    values[0].get() + values[1].get()
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic maps lower to GC storage with optional lookup and replacement", async () => {
  const source = `fn lookup[T](values: map[string, T], key: string) -> T?:
    values[key]

fn unwrap(value: i32?) -> i32:
    match value:
        actual? => actual
        nil => 0

fn main() -> i32:
    scores := {"answer": 20, "other": 2, "answer": 40}
    unwrap(lookup(scores, "answer")) + scores.len()

fn scalar_keys() -> i32:
    values := {1: 20, 2: 2, 1: 40}
    unwrap(values[1]) + values.len()

fn empty() -> i32:
    let values: map[string, i32] = {}
    values.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.map \(struct/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
  assert.match(compilation.wat, /call \$hd\.map_get/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal((instance.exports.scalar_keys as CallableFunction)(), 42);
  assert.equal((instance.exports.empty as CallableFunction)(), 0);

  assert.equal(analyze("fn main() -> void:\n    values := {}\n").diagnostics[0]?.code, "empty-map-needs-context");
  assert.equal(analyze("fn main() -> void:\n    values := {1: 1, 2: true}\n").diagnostics[0]?.code, "type-mismatch");
  assert.equal(analyze("fn main() -> void:\n    values := {1.5: 1}\n").diagnostics[0]?.code, "unsupported-map-key");
});

test("capturing closures store outer locals in GC environments", async () => {
  const source = `fn main() -> i32:
    base := 40
    add := fn(value: i32) -> i32: base + value
    add(2)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$closure0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("reference identity uses Wasm GC identity and enforces Reference bounds", async () => {
  const source = `data Box:
    value: i32

trait Value:
    fn get(self) -> i32

impl Value for Box:
    fn get(self) -> i32: self.value

enum Status:
    Ready
    Code(value: i32)

fn same[T: Reference](left: T, right: T) -> bool: left is right
fn as_value(value: Value) -> Value: value
fn pending!() -> i32: 1

fn main() -> i32:
    box := Box { value: 1 }
    alias := box
    other := Box { value: 1 }
    callback := fn() -> i32: 1
    callback_alias := callback
    other_callback := fn() -> i32: 1
    values := [1]
    values_alias := values
    first_trait := as_value(box)
    second_trait := as_value(box)
    suspension := pending()
    suspension_alias := suspension
    if same(box, alias) and not (box is other) and callback is callback_alias and not (callback is other_callback) and values is values_alias and first_trait is second_trait and suspension is suspension_alias and Status.Ready is Status.Ready and not (Status.Code(1) is Status.Code(1)):
        42
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /ref\.eq/);
  assert.match(compilation.wat, /global \$e0v0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(analyze("fn same[T](left: T, right: T) -> bool: left is right\n").diagnostics[0]?.code, "identity-needs-reference-bound");
  assert.equal(analyze("fn same[T: Reference](left: T, right: T) -> bool: left is right\nfn main() -> bool: same(1, 1)\n").diagnostics[0]?.code, "missing-trait-implementation");
});

test("heterogeneous tuples retain static element types through Wasm GC storage", async () => {
  const source = `fn load!() -> i32: 36
fn pair[T](left: T, right: i32) -> (T, i32): (left, right)

fn main!() -> i32:
    loaded, text := (load!(), "界")
    nested := ((1, 2),)
    single := (1,)
    generic := pair("value", 1)
    let first, second: (i32, i32) = (1, 2)
    first = 2
    loaded + text.len() + nested.0.0 + single.0 + generic.1 + first + second - second
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(analyze("fn same(left: (i32, i32), right: (i32, i32)) -> bool: left is right\n").diagnostics[0]?.code, "identity-requires-references");
  assert.equal(analyze("fn bad(value: (i32,)) -> i32: value.1\n").diagnostics[0]?.code, "tuple-index-range");
});

test("nested closures propagate grandparent captures through GC environments", async () => {
  const source = `fn main() -> i32:
    base := 40
    make := fn(delta: i32) -> fn(i32) -> i32:
        fn(value: i32) -> i32: base + delta + value
    add := make(1)
    add(1)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /\(type \$env1 \(struct/);
  assert.match(compilation.wat, /struct\.get \$env0 \$env0f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("closures capture lexical provider overrides when they escape $.with", async () => {
  const source = `fn make_reader() -> (fn() -> i32) $ Backup:
    $.with(Clock=$.use(Backup)):
        fn() -> i32:
            _ := $.use(Clock)
            42

fn main() -> i32 $ Backup:
    reader := make_reader()
    reader()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct\n\s*\(field \$env0f0 externref\)/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }), 42);
});

test("requirement-bearing closure types pass providers at invocation", async () => {
  const source = `fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock:
    reader := fn() -> i32 $ Clock:
        _ := $.use(Clock)
        42
    invoke(reader)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$sig0 \(func \(param anyref\) \(param externref\)/);
  assert.match(compilation.wat, /call_ref \$sig0/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);
});

test("requirement-bearing named function values receive call-site providers", async () => {
  const source = `fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    42

fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock: invoke(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$fv0[^]*param \$provider0 externref/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);
});

test("closures infer unsatisfied requirements but capture lexical providers", async () => {
  const source = `fn main() -> i32 $ Clock:
    reader := fn() -> i32:
        _ := $.use(Clock)
        42
    reader()
`;
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn()->i32$Clock");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);

  const missing = source.replace("fn main() -> i32 $ Clock:", "fn main() -> i32:");
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-requirement");
});

test("concrete requirement rows thread hidden externref providers", async () => {
  const source = `fn read() -> i32 $ Clock: 40
fn middle() -> i32 $ Clock: read() + 1
fn main() -> i32 $ Clock: middle() + 1
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$provider0 externref/);
  assert.match(compilation.wat, /call \$f0 \(local\.get \$provider0\)/);
  assert.equal((instance.exports.main as CallableFunction)({ now: 0 }), 42);
});

test("calls cannot acquire undeclared requirements", () => {
  const source = "fn read() -> i32 $ Clock: 1\nfn main() -> i32: read()\n";
  assert.equal(analyze(source).diagnostics[0]?.code, "missing-requirement");
});

test("$.use resolves hidden providers and $.with overrides them lexically", async () => {
  const source = `fn main() -> i32 $ Clock + Backup:
    _ := $.use(Clock)
    $.with(Clock=$.use(Backup)):
        _ := $.use(Clock)
        42
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /local\.set \$l0 \(local\.get \$provider0\)/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);
});

test("provider access and scope errors are diagnosed statically", () => {
  assert.equal(analyze("fn main() -> void: _ := $.use(Clock)\n").diagnostics[0]?.code, "missing-requirement");
  assert.equal(analyze("fn main() -> void:\n    $.with(Clock=1):\n        pass\n").diagnostics[0]?.code, "provider-type-mismatch");
});

test("concrete requirement rows normalize union and subtraction as sets", () => {
  const result = analyze("fn main() -> void $ Logger + Clock + Logger - Logger: pass\n");
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(result.hir?.functions[0]?.requirements, ["Clock"]);

  const grouped = analyze("fn main() -> void $ (Logger + Clock) - Logger: pass\n");
  assert.deepEqual(grouped.diagnostics, []);
  assert.deepEqual(grouped.hir?.functions[0]?.requirements, ["Clock"]);

  const empty = analyze("fn invoke(callback: fn() -> void $()) -> void: callback()\nfn main() -> void:\n    callback := fn() -> void $(): pass\n    invoke(callback)\n");
  assert.deepEqual(empty.diagnostics, []);
});

test("provider contexts use GC structs and spread into lexical call arguments", async () => {
  const source = `fn make_context() -> $.Context[Clock + Backup] $ Clock + Backup:
    $.context(Clock=$.use(Clock), Backup=$.use(Backup))

fn read() -> i32 $ Clock: 42

fn main() -> i32 $ Clock + Backup:
    context := make_context()
    $.with(Clock=$.use(Backup), ...context):
        read()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$context0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$context0/);
  assert.match(compilation.wat, /struct\.get \$context0 \$context0f0/);
  assert.match(compilation.wat, /call \$f1 \(local\.get \$l/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);
});

test("context creation accepts spreads and normalizes exact replacement keys", () => {
  const source = `fn make() -> $.Context[Clock + Backup] $ Clock + Backup:
    base := $.context(Clock=$.use(Clock))
    $.context(...base, Backup=$.use(Backup), Clock=$.use(Backup))
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const final = result.hir?.functions[0]?.body.at(-1);
  assert.equal(final?.kind, "expression");
  if (final?.kind === "expression") assert.equal(final.expression.type, "context:Backup+Clock");
});

test("context spreads and annotations are checked statically", () => {
  assert.equal(analyze("fn main() -> void:\n    $.with(...1):\n        pass\n").diagnostics[0]?.code, "context-spread-type");
  assert.equal(analyze("fn main() -> $.Context[Clock] $ Clock + Backup: $.context(Backup=$.use(Backup))\n").diagnostics[0]?.code, "type-mismatch");
});

test("suspending functions construct GC frames and bang calls drive them", async () => {
  const source = `fn add_two!(value: i32) -> i32 $ Clock:
    _ := $.use(Clock)
    value + 2

fn main!() -> i32 $ Clock:
    add_two!(40)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$s0 \(struct/);
  assert.match(compilation.wat, /\(func \$f0 .*\(result \(ref null \$s0\)\)/);
  assert.match(compilation.wat, /\(func \$poll0/);
  assert.match(compilation.wat, /struct\.set \$s0 \$s0state/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);
});

test("generic suspending functions box frame arguments and unbox direct or stored results", async () => {
  const source = `fn echo![T](value: T) -> T: value

fn main!() -> i32:
    let pending: mut Suspend[i32] = echo(20)
    left := pending!()
    right := echo!(22)
    left + right
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic suspension frames retain trait dictionaries across child polls", async () => {
  const source = `trait Score:
    fn score(self) -> i32

data Value:
    amount: i32

impl Score for Value:
    fn score(self) -> i32: self.amount

fn pause!() -> void: pass

fn read![T: Score](value: T) -> i32:
    pause!()
    value.score()

fn main!() -> i32:
    read!(Value { amount: 42 })
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.match(compilation.wat, /field \$s1b0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.get \$s1 \$s1b0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("ordinary suspending calls are cold values and bang calls need a driver", () => {
  const cold = analyze("fn work!() -> i32: 42\nfn main() -> void:\n    pending := work()\n");
  assert.equal(cold.diagnostics[0]?.code, "unused-local-binding");
  assert.equal(cold.hir?.functions[1]?.locals[0]?.type, "suspend(0):i32");
  assert.equal(analyze("fn work!() -> i32: 42\nfn main() -> i32: work!()\n").diagnostics[0]?.code, "bang-call-outside-suspension");
  assert.equal(analyze("fn plain() -> i32: 42\nfn main!() -> i32: plain!()\n").diagnostics[0]?.code, "not-suspending");
  assert.equal(analyze("fn work!() -> i32: 42\nfn main!() -> i32:\n    pending := work()\n    pending!()\n").diagnostics[0]?.code, "mutable-receiver-required");
});

test("unresolved standard task combinators have a dedicated boundary diagnostic", () => {
  const source = `fn ready!() -> i32: 42
fn main!() -> i32: all!(ready(), ready())
`;
  assert.equal(analyze(source).diagnostics[0]?.code, "unsupported-task-combinator");
  assert.equal(analyze(source.replace("all!", "race!")).diagnostics[0]?.code, "unsupported-task-combinator");

  const userDefined = `fn all!(left: i32, right: i32) -> i32: left + right
fn main!() -> i32: all!(20, 22)
`;
  assert.deepEqual(analyze(userDefined).diagnostics, []);
});

test("explicit mutable suspension bindings are one-shot", async () => {
  const valid = `fn ready!() -> i32: 42
fn main!() -> i32:
    let pending: mut Suspend[i32] = ready()
    pending!()
`;
  const { instance } = await instantiate(valid);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const secondDrive = `fn ready!() -> i32: 42
fn main!() -> i32:
    let pending: mut Suspend[i32] = ready()
    _ := pending!()
    pending!()
`;
  const second = await instantiate(secondDrive);
  assert.throws(() => (second.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("cold suspension cancellation is synchronous and idempotent", async () => {
  const source = `fn ready!() -> i32: 42
fn main() -> i32:
    let pending: mut Suspend[i32] = ready()
    pending.cancel()
    pending.cancel()
    42
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(func \$cancel0/);
  assert.match(compilation.wat, /i32\.const 3/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("cancelled suspensions cannot be driven and readonly values cannot be cancelled", async () => {
  const cancelled = `fn ready!() -> i32: 42
fn main!() -> i32:
    let pending: mut Suspend[i32] = ready()
    pending.cancel()
    pending!()
`;
  const execution = await instantiate(cancelled);
  assert.throws(() => (execution.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);

  const readonly = "fn ready!() -> i32: 42\nfn main() -> void:\n    pending := ready()\n    pending.cancel()\n";
  assert.equal(analyze(readonly).diagnostics[0]?.code, "mutable-receiver-required");
});

test("defer suites cannot suspend", () => {
  const source = `fn ready!() -> void: pass
fn main!() -> void:
    defer:
        ready!()
    pass
`;
  assert.equal(analyze(source).diagnostics[0]?.code, "suspending-defer");
});

test("suspension state transitions are observable through the trace ABI", async () => {
  const source = `fn child!() -> i32: 42
fn main!() -> i32: child!()
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, { trace: (functionIndex, event) => events.push([functionIndex, event]) });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(events, [
    [1, 0], [1, 1],
    [0, 0], [0, 1], [0, 2],
    [1, 2],
  ]);
});

test("host fixtures can keep a GC frame pending across deterministic polls", async () => {
  const source = "fn main!() -> i32: 42\n";
  const events: Array<[number, number]> = [];
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => {
      assert.equal(functionIndex, 0);
      polls.push(pollCount);
      return pollCount < 3;
    },
  });
  assert.match(compilation.wat, /field \$s0polls \(mut i32\)/);
  assert.match(compilation.wat, /call \$hd\.pending/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(polls, [1, 2, 3]);
  assert.deepEqual(events, [
    [0, 0],
    [0, 1], [0, 6],
    [0, 1], [0, 6],
    [0, 1], [0, 2],
  ]);
});

test("development drivers reject competing and reentrant suspension control", async () => {
  const source = `fn main!() -> i32: 42
`;

  const competing = await instantiate(source, { pending: () => true });
  (competing.instance.exports.__hd_start as CallableFunction)();
  assert.equal((competing.instance.exports.__hd_poll as CallableFunction)(), 0);
  assert.throws(() => (competing.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
  (competing.instance.exports.__hd_cancel as CallableFunction)();

  let pollingInstance: WebAssembly.Instance;
  let nestedPollError: unknown;
  const polling = await instantiate(source, {
    pending: () => {
      try {
        (pollingInstance.exports.__hd_poll as CallableFunction)();
      } catch (error) {
        nestedPollError = error;
      }
      return false;
    },
  });
  pollingInstance = polling.instance;
  (pollingInstance.exports.__hd_start as CallableFunction)();
  assert.equal((pollingInstance.exports.__hd_poll as CallableFunction)(), 1);
  assert.ok(nestedPollError instanceof WebAssembly.RuntimeError);

  let cancellingInstance: WebAssembly.Instance;
  let nestedCancelError: unknown;
  const cancelling = await instantiate(source, {
    pending: () => {
      try {
        (cancellingInstance.exports.__hd_cancel as CallableFunction)();
      } catch (error) {
        nestedCancelError = error;
      }
      return false;
    },
  });
  cancellingInstance = cancelling.instance;
  (cancellingInstance.exports.__hd_start as CallableFunction)();
  assert.equal((cancellingInstance.exports.__hd_poll as CallableFunction)(), 1);
  assert.ok(nestedCancelError instanceof WebAssembly.RuntimeError);
});

test("child pending propagates through the parent frame and restores locals", async () => {
  const source = `fn child!() -> i32: 2
fn main!() -> i32:
    base := 40
    value := child!()
    base + value
`;
  const events: Array<[number, number]> = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.match(compilation.wat, /field \$s1l0 \(mut i32\)/);
  assert.match(compilation.wat, /field \$s1child0 \(mut \(ref null \$s0\)\)/);
  assert.match(compilation.wat, /call \$poll0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(events, [
    [1, 0], [1, 1],
    [0, 0], [0, 1], [0, 6], [1, 6],
    [1, 1], [0, 1], [0, 2], [1, 2],
  ]);

  const nested = "fn child!() -> i32: 41\nfn main!() -> i32: child!() + 1\n";
  const nestedResult = await instantiate(nested, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((nestedResult.instance.exports.main as CallableFunction)(), 42);
});

test("started-frame cancellation cancels the child and runs registered cleanup", async () => {
  const source = `fn child!() -> i32: 2
fn main!() -> i32:
    defer:
        pass
    value := child!()
    value
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex === 0,
  });
  const start = instance.exports.__hd_start as CallableFunction;
  const poll = instance.exports.__hd_poll as CallableFunction;
  const cancel = instance.exports.__hd_cancel as CallableFunction;
  start();
  assert.equal(poll(), 0);
  cancel();
  assert.deepEqual(events, [
    [1, 0], [1, 1],
    [0, 0], [0, 1], [0, 6], [1, 6],
    [1, 3], [0, 3], [1, 7],
  ]);
  cancel();
  assert.deepEqual(events.at(-1), [1, 3]);
  assert.throws(() => poll(), WebAssembly.RuntimeError);
});

test("cancellation unwinds child frames before parent cleanup", async () => {
  const source = `fn wait!() -> void: pass

fn leaf!() -> void:
    defer:
        pass
    wait!()

fn middle!() -> void:
    defer:
        pass
    leaf!()

fn main!() -> void:
    defer:
        pass
    middle!()
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex === 0,
  });
  (instance.exports.__hd_start as CallableFunction)();
  assert.equal((instance.exports.__hd_poll as CallableFunction)(), 0);
  (instance.exports.__hd_cancel as CallableFunction)();
  assert.deepEqual(events.slice(-7), [
    [3, 3], [2, 3], [1, 3], [0, 3],
    [1, 7], [2, 7], [3, 7],
  ]);
});

test("linear suspension frames resume across multiple child sites", async () => {
  const source = `fn left!(value: i32) -> i32: value
fn right!(value: i32) -> i32: value
fn main!() -> i32:
    base := 1
    left_value := left!(20)
    middle := 1
    right_value := right!(20)
    base + left_value + middle + right_value
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex < 2 && pollCount === 1,
  });
  assert.match(compilation.wat, /field \$s2child0 \(mut \(ref null \$s0\)\)/);
  assert.match(compilation.wat, /field \$s2child1 \(mut \(ref null \$s1\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves nested expression evaluation", async () => {
  const source = `fn value!(input: i32) -> i32: input

fn main!() -> i32:
    value!(20) + value!(22)
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.doesNotMatch(compilation.wat, /unsupported-nested-suspension/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves nested argument order", async () => {
  const source = `fn value!(input: i32) -> i32: input
fn add!(left: i32, right: i32) -> i32: left + right

fn main!() -> i32:
    add!(value!(20), value!(22))
`;
  const polls: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => {
      polls.push([functionIndex, pollCount]);
      return functionIndex === 0 && pollCount === 1;
    },
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(polls.filter(([functionIndex]) => functionIndex === 0), [[0, 1], [0, 2], [0, 1], [0, 2]]);
  assert.deepEqual(polls.filter(([functionIndex]) => functionIndex === 1), [[1, 1]]);
  assert.deepEqual(polls.filter(([functionIndex]) => functionIndex === 2), [[2, 1], [2, 2], [2, 3]]);
});

test("CFG suspension lowering branches and short-circuits around child frames", async () => {
  const source = `fn value!(input: i32) -> i32: input

fn choose!(flag: bool) -> i32:
    if flag and value!(1) == 1:
        value!(20) + value!(22)
    else:
        value!(99)

fn main!() -> i32: choose!(true)
`;
  const polls: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => {
      polls.push([functionIndex, pollCount]);
      return functionIndex === 0 && pollCount === 1;
    },
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(polls.filter(([functionIndex]) => functionIndex === 0), [
    [0, 1], [0, 2],
    [0, 1], [0, 2],
    [0, 1], [0, 2],
  ]);
});

test("CFG suspension lowering preserves loops, continue, break values, and cleanup", async () => {
  const source = `fn step!(value: i32) -> i32: value

fn main!() -> i32:
    let index: i32 = 0
    let total: i32 = 0
    value := while index < 5:
        defer:
            total = total + 10
        index = step!(index + 1)
        if index == 2:
            continue
        total = total + step!(index)
        if index == 4:
            break total
    else:
        0
    value + total - 44
`;
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves match bindings and suspending guards", async () => {
  const source = `enum Number:
    Value(value: i32)
    Empty

fn number!(value: Number) -> Number: value
fn probe!(value: i32) -> i32: value

fn main!() -> i32:
    match number!(Number.Value(20)):
        Number.Value(value) if probe!(value) == 20 => probe!(value) + 22
        Number.Value(value) => value
        Number.Empty => 0
`;
  const { instance } = await instantiate(source, {
    pending: (_functionIndex, pollCount) => pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering propagates Result failures after child completion", async () => {
  const source = `data ParseError:
    code: i32

fn parse!(ok: bool) -> Result[i32, ParseError]:
    if ok:
        Ok(40)
    else:
        Err(ParseError { code: 7 })

fn lifted!(ok: bool) -> Result[i32, ParseError]:
    value := parse!(ok)?
    Ok(value + 2)

fn inspect(value: Result[i32, ParseError]) -> i32:
    match value:
        Ok(actual) => actual
        Err(error) => -error.code

fn main!() -> i32:
    inspect(lifted!(true)) + inspect(lifted!(false))
`;
  const { instance } = await instantiate(source, {
    pending: (_functionIndex, pollCount) => pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 35);
});

test("CFG suspension cancellation cancels the active child and runs scoped cleanup", async () => {
  const source = `fn child!() -> i32: 2

fn main!() -> i32:
    if true:
        defer:
            pass
        child!() + 40
    else:
        0
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex === 0,
  });
  const start = instance.exports.__hd_start as CallableFunction;
  const poll = instance.exports.__hd_poll as CallableFunction;
  const cancel = instance.exports.__hd_cancel as CallableFunction;
  start();
  assert.equal(poll(), 0);
  cancel();
  assert.deepEqual(events, [
    [1, 0], [1, 1],
    [0, 0], [0, 1], [0, 6], [1, 6],
    [1, 3], [0, 3], [1, 7],
  ]);
  assert.throws(() => poll(), WebAssembly.RuntimeError);
});

test("CFG suspension lowering installs providers produced after resumption", async () => {
  const source = `trait Clock

data MockClock:
    value: i32

impl Clock for MockClock

fn make_clock!(value: i32) -> Clock:
    MockClock { value: value }

fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    42

fn main!() -> i32:
    $.with(Clock=make_clock!(1)):
        read()
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.match(compilation.wat, /struct\.new \$trait0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering nests dynamic trait suspensions", async () => {
  const source = `trait Reader:
    fn read!(self, value: i32) -> i32

data FixedReader:
    offset: i32

impl Reader for FixedReader:
    fn read!(self, value: i32) -> i32: self.offset + value

fn combine!(reader: Reader) -> i32:
    reader.read!(20) + reader.read!(22)

fn main!() -> i32:
    combine!(FixedReader { offset: 0 })
`;
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("suspension poll decisions record and replay with configuration identity", async () => {
  const source = `fn child!() -> i32: 2
fn main!() -> i32:
    value := child!()
    value + 40
`;
  const events: ReplayEvent[] = [];
  const recorded = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
    record: (event) => events.push(event),
    providerConfigurationId: "fixture-a",
  });
  assert.equal((recorded.instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(events.map((event) => event.siteId), ["main:poll:1", "child:poll:1", "main:poll:2", "child:poll:2"]);
  assert.equal(events[1]?.encodedResult, "pending");

  const replayed = await instantiate(source, {
    pending: () => { throw new Error("live pending callback must not run during replay"); },
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.equal((replayed.instance.exports.main as CallableFunction)(), 42);
  replayed.replay.assertComplete();
  assert.equal(replayed.replay.consumed, events.length);

  const mismatched = await instantiate(source, { replay: events, providerConfigurationId: "fixture-b" });
  assert.throws(() => (mismatched.instance.exports.main as CallableFunction)(), /provider configuration/);

  const reordered = await instantiate(`fn unused() -> i32: 0\n${source}`, {
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.equal((reordered.instance.exports.main as CallableFunction)(), 42);
  reordered.replay.assertComplete();
  assert.notEqual(reordered.compilation.hir.functions.find((declaration) => declaration.name === "child")?.index, events[1]?.functionIndex);

  const changed = await instantiate(source.replace("fn child!() -> i32: 2", "fn child!() -> i32: 3"), {
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.throws(() => (changed.instance.exports.main as CallableFunction)(), /function code identity/);
});

test("println requires Console and streams displayed UTF-8 through the host boundary", async () => {
  const source = `fn main() -> i32 $ Console:
    println("value \${42}")
    println(true)
    println('λ')
    42
`;
  const lines: string[] = [];
  const providers: unknown[] = [];
  const provider = { name: "test-console" };
  const { instance, compilation } = await instantiate(source, {
    console: (text, usedProvider) => {
      lines.push(text);
      providers.push(usedProvider);
    },
  });
  assert.equal((instance.exports.main as CallableFunction)(provider), 42);
  assert.deepEqual(lines, ["value 42", "true", "λ"]);
  assert.deepEqual(providers, [provider, provider, provider]);
  assert.match(compilation.wat, /import "hd" "console_byte"/);
  assert.match(compilation.wat, /func \$hd\.console_print/);

  assert.ok(analyze(`fn main() -> void:
    println("missing")
`).diagnostics.some((diagnostic) => diagnostic.code === "missing-requirement"));
});

test("multi-provider use preserves requested tuple order through Wasm GC", async () => {
  const source = `trait Left:
    fn value(self) -> i32

trait Right:
    fn value(self) -> i32

data Number:
    value: i32

impl Left for Number:
    fn value(self) -> i32: self.value

impl Right for Number:
    fn value(self) -> i32: self.value

fn main() -> i32:
    $.with(Left=Number { value: 20 }, Right=Number { value: 22 }):
        left, right := $.use(Left, Right)
        left.value() + right.value()
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list 2/);
});

test("imported block_on drives stored suspensions and rejects nested drivers", async () => {
  const source = `use std.task.block_on

fn ready!() -> i32: 42

fn main() -> i32:
    let pending: mut Suspend[i32] = ready()
    block_on(pending)
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /global \$hd\.driver-active/);

  const nested = `use std.task.block_on

fn ready!() -> i32: 42

fn helper() -> i32:
    let pending: mut Suspend[i32] = ready()
    block_on(pending)

fn main!() -> void:
    _ := helper()
`;
  const nestedExecution = await instantiate(nested);
  assert.throws(() => (nestedExecution.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);

  const forbidden = analyze(`use std.task.block_on
fn ready!() -> i32: 42
fn main() -> void:
    let pending: mut Suspend[i32] = ready()
    defer:
        _ := block_on(pending)
`);
  assert.ok(forbidden.diagnostics.some((diagnostic) => diagnostic.code === "suspension-forbidden-context"));
});

test("imported assert_equal compares supported structural values", async () => {
  const source = `use std.testing.assert_equal

fn main() -> i32:
    assert_equal((1, ["a", "b"]), (1, ["a", "b"]), reason="same structure")
    42
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const failure = await instantiate(`use std.testing.assert_equal
fn main() -> void: assert_equal([1], [2], reason="different")
`);
  assert.throws(() => (failure.instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);

  const unsupported = analyze(`use std.testing.assert_equal
data Value: pass
fn main() -> void: assert_equal(Value {}, Value {}, reason="not derived")
`);
  assert.ok(unsupported.diagnostics.some((diagnostic) => diagnostic.code === "missing-partial-eq"));
});
