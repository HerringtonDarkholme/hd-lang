import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";
import { speculate } from "./call-speculation.ts";

test("trials restore the complete reachable graph without replacing local identities", () => {
  const local = { type: "i32", mutable: false };
  const symbol = Symbol("pending");
  const state = {
    locals: [local],
    scopes: [new Map([["value", local]])],
    closures: [] as unknown[],
    captures: new Set([local]),
    inferred: undefined as string | undefined,
    [symbol]: local,
    frozen: Object.freeze({ value: 1 }),
  };
  assert.throws(() =>
    speculate(state, () => {
      local.type = "bool";
      local.mutable = true;
      state.locals.push({ type: "bool", mutable: true });
      state.scopes[0]!.clear();
      state.scopes.push(new Map());
      state.captures.clear();
      state.closures.push({ captures: [local] });
      state.inferred = "bool";
      Object.assign(state, { temporary: true });
      state[symbol] = { type: "string", mutable: true };
      throw new Error("failed candidate");
    }),
  );
  assert.deepEqual(local, { type: "i32", mutable: false });
  assert.equal(state.locals.length, 1);
  assert.equal(state.locals[0], local);
  assert.equal(state.scopes.length, 1);
  assert.equal(state.scopes[0]!.get("value"), local);
  assert.deepEqual([...state.captures], [local]);
  assert.deepEqual(state.closures, []);
  assert.equal(state.inferred, undefined);
  assert.equal(state[symbol], local);
  assert.equal(Object.hasOwn(state, "temporary"), false);
});

test("nested successful trials restore their own entry states", () => {
  const state = { local: { type: "i32" }, cache: new Map<string, string>() };
  const result = speculate(state, () => {
    state.local.type = "bool";
    state.cache.set("outer", "bool");
    speculate(state, () => {
      state.local.type = "string";
      state.cache.clear();
    });
    assert.equal(state.local.type, "bool");
    assert.equal(state.cache.get("outer"), "bool");
    return 42;
  });
  assert.equal(result, 42);
  assert.equal(state.local.type, "i32");
  assert.equal(state.cache.size, 0);
});

test("cyclic graphs restore aliases and opaque weak state fails before checking", () => {
  const state: { value: number; self?: object; aliases: Map<object, object> } = {
    value: 1,
    aliases: new Map(),
  };
  state.self = state;
  state.aliases.set(state, state);
  speculate(state, () => {
    state.value = 2;
    state.self = {};
    state.aliases.clear();
  });
  assert.equal(state.value, 1);
  assert.equal(state.self, state);
  assert.equal(state.aliases.get(state), state);
  for (const opaque of [new WeakMap(), new WeakSet()]) {
    let entered = false;
    assert.throws(
      () =>
        speculate({ opaque }, () => {
          entered = true;
        }),
      /opaque weak collections/,
    );
    assert.equal(entered, false);
  }
});

const declarations = `trait Apply[T]:
    fn apply(self, run: fn(T) -> T) -> i32
data Item:
    value: i32
`;
const integerImplementation = `impl Apply[i32] for Item:
    fn apply(self, run: fn(i32) -> i32) -> i32:
        run(self.value)
`;
const booleanImplementation = `impl Apply[bool] for Item:
    fn apply(self, run: fn(bool) -> bool) -> i32:
        if run(true): 1 else: 0
`;

for (const reverse of [false, true]) {
  const implementations = reverse
    ? booleanImplementation + integerImplementation
    : integerImplementation + booleanImplementation;
  test(`contextually typed closure selects one candidate and commits once (reverse=${reverse})`, async () => {
    const source = `${declarations}${implementations}
fn main() -> i32:
    item := Item { value: 40 }
    offset := 2
    item.apply(fn(value): value + offset)
`;
    const result = analyze(source);
    assert.deepEqual(result.diagnostics, []);
    const closures = result.hir!.closures.filter((closure) =>
      closure.captures.some((capture) => capture.source.name === "offset"),
    );
    assert.equal(closures.length, 1);
    assert.equal(closures[0]!.parameters[0]!.type, "i32");
    assert.equal(closures[0]!.captures.length, 1);
    const { instance } = await instantiate(source);
    assert.equal((instance.exports.main as CallableFunction)(), 42);
  });

  test(`nested binding and branch scopes are isolated between candidates (reverse=${reverse})`, async () => {
    const source = `${declarations}${implementations}
fn main() -> i32:
    item := Item { value: 40 }
    item.apply(fn(value):
        local := value + 2
        if local == 42: local else: 0
    )
`;
    const result = analyze(source);
    assert.deepEqual(result.diagnostics, []);
    const closures = result.hir!.closures.filter((closure) =>
      closure.locals.some((local) => local.name === "local"),
    );
    assert.equal(closures.length, 1);
    assert.deepEqual(
      closures[0]!.locals.map((local) => local.name),
      ["value", "local"],
    );
    const { instance } = await instantiate(source);
    assert.equal((instance.exports.main as CallableFunction)(), 42);
  });
}

test("complex argument shapes do not turn two distinct traits into overloads", () => {
  const source = `${declarations}${integerImplementation}
trait Other:
    fn apply(self, run: fn(bool) -> bool) -> i32
impl Other for Item:
    fn apply(self, run: fn(bool) -> bool) -> i32: 0
fn main() -> i32:
    item := Item { value: 40 }
    item.apply(fn(value): value + 2)
`;
  assert.deepEqual(
    analyze(source).diagnostics.map((diagnostic) => diagnostic.code),
    ["ambiguous-method"],
  );
});

test("associated calls check contextually typed closures and named arguments per candidate", async () => {
  const source = `trait Build[T]:
    fn build(run: fn(T) -> T) -> Item
data Item:
    value: i32
impl Build[bool] for Item:
    fn build(run: fn(bool) -> bool) -> Item:
        Item { value: if run(true): 1 else: 0 }
impl Build[i32] for Item:
    fn build(run: fn(i32) -> i32) -> Item:
        Item { value: run(40) }
fn main() -> i32:
    Item::build(run=fn(value): value + 2).value
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("a suspending argument is trial-checked without syntax-based ambiguity", async () => {
  const source = `trait Pick[T]:
    fn pick(self, value: T) -> i32
data Item: pass
impl Pick[bool] for Item:
    fn pick(self, value: bool) -> i32: 0
impl Pick[i32] for Item:
    fn pick(self, value: i32) -> i32: value
fn child!() -> i32: 42
fn main!() -> i32:
    Item {}.pick(child!())
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source, { pending: (_index, count) => count === 1 });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("provider scopes and captures created during a failed trial do not leak", async () => {
  const source = `${declarations}${booleanImplementation}${integerImplementation}
trait Clock:
    fn now(self) -> i32
data Fixed:
    value: i32
impl Clock for Fixed:
    fn now(self) -> i32: self.value
fn main() -> i32:
    item := Item { value: 40 }
    item.apply(fn(value):
        $.with(Clock=Fixed { value: 2 }):
            clock := $.use(Clock)
            value + clock.now()
    )
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const main = result.hir!.functions.find((fn) => fn.name === "main")!;
  assert.deepEqual(main.requirements, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic method substitutions and inferred closure results are candidate-local", async () => {
  const source = `trait Apply[T]:
    fn apply[U](self, run: fn(T) -> U) -> U
data Item:
    value: i32
impl Apply[bool] for Item:
    fn apply[U](self, run: fn(bool) -> U) -> U: run(true)
impl Apply[i32] for Item:
    fn apply[U](self, run: fn(i32) -> U) -> U: run(self.value)
fn main() -> i32:
    Item { value: 40 }.apply(fn(value): value + 2)
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

for (const associated of [false, true]) {
  test(`explicit method type arguments contextualize each trial (associated=${associated})`, async () => {
    const receiver = associated ? "" : "self, ";
    const source = `trait Choose[T]:
    fn choose[U](${receiver}run: fn(T) -> U) -> U
data Item: pass
impl Choose[bool] for Item:
    fn choose[U](${receiver}run: fn(bool) -> U) -> U: run(true)
impl Choose[i32] for Item:
    fn choose[U](${receiver}run: fn(i32) -> U) -> U: run(40)
fn main() -> i32:
    ${associated ? "Item::choose" : "Item {}.choose"}::[i32](run=fn(value): value + 2)
`;
    assert.deepEqual(analyze(source).diagnostics, []);
    const { instance } = await instantiate(source);
    assert.equal((instance.exports.main as CallableFunction)(), 42);
  });
}

test("associated tuple spread trials roll back their locals and prechecked rewrites", async () => {
  const source = `trait Build[T]:
    fn build(first: T, second: T) -> Item
data Item:
    value: i32
impl Build[bool] for Item:
    fn build(first: bool, second: bool) -> Item: Item { value: 0 }
impl Build[i32] for Item:
    fn build(first: i32, second: i32) -> Item: Item { value: first + second }
fn main() -> i32:
    pair := (40, 2)
    Item::build(pair...).value
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const main = result.hir!.functions.find((fn) => fn.name === "main")!;
  assert.equal(main.locals.filter((local) => local.name.startsWith("$spread")).length, 2);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("associated candidate selection includes the expected result type", async () => {
  const source = `trait Make[T]:
    fn make() -> T
data Item: pass
impl Make[bool] for Item:
    fn make() -> bool: false
impl Make[i32] for Item:
    fn make() -> i32: 42
fn main() -> i32:
    Item::make()
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("associated numeric candidates use the ordinary literal-default tie break", async () => {
  const source = `trait Build[T]:
    fn build(value: T) -> Item
data Item:
    value: i32
impl Build[i64] for Item:
    fn build(value: i64) -> Item: Item { value: 0 }
impl Build[i32] for Item:
    fn build(value: i32) -> Item: Item { value: value }
fn main() -> i32:
    Item::build(value=42).value
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("failed associated fits list instantiations, without candidate diagnostics", () => {
  const source = `trait Build[T]:
    fn build(value: T) -> Item
data Item: pass
impl Build[bool] for Item:
    fn build(value: bool) -> Item: Item {}
impl Build[i32] for Item:
    fn build(value: i32) -> Item: Item {}
fn main() -> Item:
    Item::build(value="wrong")
`;
  const diagnostics = analyze(source).diagnostics;
  assert.equal(diagnostics.length, 1);
  assert.equal(diagnostics[0]!.code, "type-mismatch");
  assert.match(diagnostics[0]!.message, /available: Build\[bool\], Build\[i32\]/);
});

test("lazy result inference state is restored with signature entries between trials", async () => {
  const source = `trait Pick[T]:
    fn pick(self, value: T) -> i32
data Item: pass
impl Pick[bool] for Item:
    fn pick(self, value: bool) -> i32: 0
impl Pick[i32] for Item:
    fn pick(self, value: i32) -> i32: value
fn main() -> i32:
    Item {}.pick(inferred())
fn inferred(): 42
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.hir!.functions.find((fn) => fn.name === "inferred")!.result, "i32");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
