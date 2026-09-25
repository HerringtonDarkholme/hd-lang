import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compile, instantiate } from "../src/compiler.ts";

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
  assert.equal(
    analyze("fn main() -> i32: +2147483648\n").diagnostics[0]?.code,
    "integer-literal-range",
  );
  const division = await instantiate("fn main() -> i32: -2147483648 / -1\n");
  assert.throws(
    () => (division.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
});

test("integer power is right-associative, checked, and rejects negative exponents", async () => {
  const { instance, compilation } = await instantiate("fn main() -> i32: -2 ** 2 + 2 ** 3 ** 2\n");
  assert.match(compilation.wat, /call \$hd\.pow_i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 508);

  const negative = await instantiate("fn main() -> i32: 2 ** -1\n");
  assert.throws(
    () => (negative.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
  const overflow = await instantiate("fn main() -> i32: 2 ** 31\n");
  assert.throws(
    () => (overflow.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
});

test("floating power uses the host IEEE pow primitive", async () => {
  const { instance, compilation } = await instantiate("fn main() -> f64: 9.0 ** 0.5\n");
  assert.match(compilation.wat, /import "hd" "pow_f64"/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
  assert.equal(analyze("fn main() -> f64: 2 ** 2.0\n").diagnostics[0]?.code, "mixed-numeric-types");
});

test("checker rejects name, mutability, and type errors", () => {
  assert.equal(analyze("fn main() -> i32: missing\n").diagnostics[0]?.code, "unknown-name");
  assert.equal(
    analyze("fn main() -> i32:\n    x := 1\n    x = 2\n    x\n").diagnostics[0]?.code,
    "non-reassignable-binding",
  );
  assert.equal(analyze("fn main() -> i32: true\n").diagnostics[0]?.code, "type-mismatch");
  assert.equal(
    analyze("fn main() -> f64: 2.0 % 1.0\n").diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze("fn main() -> bool: true < false\n").diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze("fn bad(first: i32..., second: i32) -> i32: second\n").diagnostics[0]?.code,
    "nonfinal-vararg",
  );
  assert.equal(
    analyze(
      "fn fixed(value: i32) -> i32: value\nfn main(values: list[i32]) -> i32: fixed(values...)\n",
    ).diagnostics[0]?.code,
    "positional-spread-needs-vararg",
  );
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
  assert.equal(
    analyze(`fn fixed(value: list[i32]) -> i32: value.len()
fn apply_callback(callback: fn(i32...) -> i32) -> i32: callback()
fn main() -> i32: apply_callback(fixed)
`).diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$parameter-default.combine.second",
    ),
  );
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$parameter-default.choose.fallback",
    ),
  );
});

test("function parameter defaults enforce order, type, and purity", () => {
  assert.equal(
    analyze("fn bad(first: i32 = 1, second: i32) -> i32: second\n").diagnostics[0]?.code,
    "parameter-default-order",
  );
  assert.equal(
    analyze("fn bad(value: i32 = true) -> i32: value\n").diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze(`fn wait!() -> i32: 1
fn bad(value: i32 = wait!()) -> i32: value
`).diagnostics[0]?.code,
    "impure-parameter-default",
  );
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
  const ordinary =
    await instantiate(`fn combine(first: i32, second: i32) -> i32: first * 10 + second
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

  assert.equal(
    analyze(`enum Pair:
    Value(first: i32, second: i32)
fn main() -> Pair: Pair.Value(missing=1, first=2)
`).diagnostics[0]?.code,
    "unknown-named-argument",
  );
  assert.equal(
    analyze(`enum Pair:
    Value(first: i32, second: i32)
fn main() -> Pair: Pair.Value(1, first=2)
`).diagnostics[0]?.code,
    "duplicate-argument",
  );
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
  assert.equal(
    analyze(`enum Choice:
    Pair(left: i32, right: i32)
fn bad(choice: Choice) -> i32:
    match choice:
        Choice.Pair(missing=value, right=other) => value
`).diagnostics[0]?.code,
    "unknown-variant-pattern-field",
  );
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
  assert.equal(
    analyze(`enum Expr:
    Scale(value: i32, factor: i32)
fn incomplete(expr: Expr) -> i32:
    match expr:
        Expr.Scale(value, factor=2) => value
`).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
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
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$enum-variant.Status.Unknown",
    ),
  );
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$enum-default.Status.doubled",
    ),
  );
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
  assert.equal(
    analyze("fn main() -> i32:\n    if true:\n        hidden := 1\n    hidden\n").diagnostics[0]
      ?.code,
    "unknown-name",
  );
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
  assert.equal(
    analyze("fn main() -> void:\n    while true:\n        break 1\n").diagnostics[0]?.code,
    "break-value-context",
  );
  assert.equal(
    analyze("fn main() -> i32:\n    while true:\n        break\n    else:\n        1\n")
      .diagnostics[0]?.code,
    "break-value-context",
  );
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
  assert.equal(
    analyze("fn bad() -> void:\n    for value in 1:\n        pass\n").diagnostics[0]?.code,
    "not-iterable",
  );
  assert.equal(
    analyze("fn bad(values: list[i32]) -> void:\n    for left, right in values:\n        pass\n")
      .diagnostics[0]?.code,
    "for-binding-arity",
  );
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

  assert.equal(
    analyze('data User:\n    name: string\nfn bad(user: User) -> void: user.name = "x"\n')
      .diagnostics[0]?.code,
    "readonly-root",
  );
  assert.equal(
    analyze(
      'data Child:\n    name: string\ndata Parent:\n    child: Child\nfn bad(parent: mut Parent) -> void: parent.child.name = "x"\n',
    ).diagnostics[0]?.code,
    "readonly-edge",
  );
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
  assert.ok(
    analyze(`fn bad(values: list[i32]) -> void:
    values.append(1)
`).diagnostics.some((diagnostic) => diagnostic.code === "mutable-receiver-required"),
  );
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

  assert.equal(
    analyze(`fn bad(values: map[string, i32]) -> void:
    _ := values.remove("x")
`).diagnostics[0]?.code,
    "mutable-receiver-required",
  );
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
    if (firstBinding.value.kind === "data")
      assert.deepEqual(firstBinding.value.fieldIndices, [0, 1, 2]);
  }
});

test("data field defaults are type checked and purity checked", () => {
  assert.equal(
    analyze(`data Box[T]:
    value: T? = nil
fn make() -> Box[i32]: Box {}
`).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(`fn wait!() -> i32: 1
data Bad:
    value: i32 = wait!()
`).diagnostics[0]?.code,
    "impure-data-default",
  );
  assert.equal(
    analyze(`data Bad:
    value: i32 = true
`).diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.equal(
    analyze(`data Other:
    value: i32
data Counter:
    value: i32
fn bad(other: Other) -> Counter: Counter { ...other }
`).diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.equal(
    analyze(`${header}fn main() -> i32: Point { y: 1 }.x\n`).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(`${header}fn main() -> i32: Point { }.x\n`).diagnostics[0]?.code,
    "missing-required-field",
  );
  assert.equal(
    analyze(`${header}fn main() -> i32: Point { x: true }.x\n`).diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.equal(
    analyze(
      `${header}fn read(user: User) -> i32:\n    match user:\n        User { age, age=other } => age\n`,
    ).diagnostics[0]?.code,
    "duplicate-data-pattern-field",
  );
  assert.equal(
    analyze(
      `${header}fn read(user: User) -> i32:\n    match user:\n        User { missing } => 1\n`,
    ).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(`${header}fn read(user: User) -> i32:\n    match user:\n        User { age=1 } => 1\n`)
      .diagnostics[0]?.code,
    "nonexhaustive-match",
  );
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
  assert.equal(
    analyze(
      `${header}fn main(flag: Flag) -> i32:\n    match flag:\n        Flag.On(value) => value\n`,
    ).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      `${header}fn main(flag: Flag) -> i32:\n    match flag:\n        Flag.On => 1\n        Flag.Off => 0\n`,
    ).diagnostics[0]?.code,
    "pattern-arity",
  );
  assert.equal(
    analyze(
      `${header}fn main(flag: Flag) -> i32:\n    match flag:\n        _ => 1\n        Flag.Off => 0\n`,
    ).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
  assert.equal(
    analyze(
      `${header}fn main(flag: Flag) -> i32:\n    match flag:\n        On => 1\n        .Off => 0\n`,
    ).diagnostics[0]?.code,
    "bare-variant-pattern",
  );
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
  assert.ok(
    swapped.diagnostics.some(
      (diagnostic) =>
        diagnostic.code === "variant-binding-name-mismatch" && diagnostic.severity === "warning",
    ),
  );
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
  assert.equal(
    analyze("enum Status:\n    Queued\nfn main() -> void:\n    status := .Queued\n").diagnostics[0]
      ?.code,
    "missing-contextual-enum-type",
  );
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
  assert.equal(
    analyze("fn choose(flag: bool) -> i32:\n    match flag:\n        true => 1\n").diagnostics[0]
      ?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      "fn choose(flag: bool) -> i32:\n    match flag:\n        true => 1\n        true => 2\n        false => 0\n",
    ).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
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
  assert.equal(
    analyze("fn classify(value: i32) -> i32:\n    match value:\n        1 => 1\n").diagnostics[0]
      ?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      "fn classify(value: char) -> i32:\n    match value:\n        'x' => 1\n        'x' => 2\n        _ => 0\n",
    ).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
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
  assert.equal(
    analyze("fn classify(value: bool) -> i32:\n    match value:\n        _ if true => 1\n")
      .diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      "fn classify(value: bool) -> i32:\n    match value:\n        candidate if candidate => 1\n        _ => 0\n",
    ).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(
      "fn classify(value: bool) -> i32:\n    match value:\n        _ if 1 => 1\n        _ => 0\n",
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.equal(
    analyze("value := Ok(1)\n").diagnostics[0]?.code,
    "result-constructor-needs-context",
  );
  assert.equal(
    analyze("fn main() -> i32:\n    value := 1\n    value?\n").diagnostics[0]?.code,
    "invalid-result-propagation",
  );
  const nested = analyze("fn main() -> void:\n    let nested: string?? = nil\n");
  assert.ok(nested.hir);
  assert.equal(nested.diagnostics[0]?.code, "unused-local-binding");
});

test("optional and Result values are must-use unless explicitly discarded", () => {
  const prefix = "fn maybe() -> i32?: nil\n";
  assert.equal(
    analyze(`${prefix}fn main() -> void: maybe()\n`).diagnostics[0]?.code,
    "discarded-must-use-value",
  );
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
  assert.equal(
    analyze("fn main() -> i32:\n    apply := fn(value: i32) -> i32: value\n    apply(true)\n")
      .diagnostics[0]?.code,
    "type-mismatch",
  );
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
  assert.equal(
    analyze("fn main() -> i32:\n    closure := fn(value) -> i32: value\n    0\n").diagnostics[0]
      ?.code,
    "closure-parameter-needs-annotation",
  );
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
  assert.equal(
    analyze(
      "fn main() -> i32:\n    bad := fn(value: i32):\n        if value > 0:\n            return 1\n        return true\n    0\n",
    ).diagnostics[0]?.code,
    "closure-result-type",
  );
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
