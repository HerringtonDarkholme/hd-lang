import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compileToWat, instantiate, type ReplayEvent } from "../compiler.ts";
import { RuntimePanicError } from "../runtime-panic.ts";
import { calledTraitMethods, reachableProgram, traitMethodKey } from "./reachability.ts";

test("empty programs do not emit unused standard functions or dictionaries", async () => {
  const source = "pub fn main() -> void: pass\n";
  const compilation = compileToWat(source);
  const live = reachableProgram(compilation.hir);
  assert.ok(compilation.hir.functions.length > 100);
  assert.ok(live.functions.length < 10);
  assert.equal(live.closures.length, 0);
  assert.equal(live.implementations.length, 0);
  for (const declaration of compilation.hir.functions) {
    if (live.functions.includes(declaration) || declaration.intrinsicMethod) continue;
    assert.ok(!compilation.wat.includes(`(func $f${declaration.index} `), declaration.name);
  }
  const { instance } = await instantiate(source);
  (instance.exports.main as CallableFunction)();
});

test("test roots survive but unused standard default-argument helpers do not", async () => {
  const source = 'tests:\n    it("one"):\n        pass\n';
  const compilation = compileToWat(source);
  const live = reachableProgram(compilation.hir);
  assert.equal(live.functions.filter((item) => item.testOptions).length, 1);
  assert.ok(!live.functions.some((item) => item.name.startsWith("$parameter-default.")));
  await instantiate(source);
});

test("standard function values retain generic dictionaries and transitively called helpers", async () => {
  const source = `use std.cmp.min as smallest
fn invoke(callback: fn(i32, i32) -> i32) -> i32:
    callback(42, 100)
fn main() -> i32:
    invoke(smallest)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("reachable closure captures retain nested code while unused standard closures disappear", async () => {
  const source = `fn factory(value: i32) -> fn() -> i32:
    fn() -> i32: value
fn main() -> i32:
    callback := factory(42)
    callback()
`;
  const { instance, compilation } = await instantiate(source);
  const live = reachableProgram(compilation.hir);
  assert.equal(live.closures.length, 1);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("default argument references retain their helper and its call dependencies", async () => {
  const source = `fn answer() -> i32: 42
fn choose(value: i32 = answer()) -> i32: value
fn main() -> i32: choose()
`;
  const checked = analyze(source);
  assert.ok(checked.hir);
  // Model library provenance without adding artificial declarations to std.
  const program = {
    ...checked.hir,
    functions: checked.hir.functions.map((item) =>
      ["answer", "choose", "$parameter-default.choose.value"].includes(item.name)
        ? { ...item, standard: true as const }
        : item,
    ),
  };
  const live = reachableProgram(program);
  for (const name of ["answer", "choose", "$parameter-default.choose.value"])
    assert.ok(live.functions.some((item) => item.name === name));
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("unreferenced user functions remain host-callable roots", async () => {
  const source = "fn main() -> i32: 0\nfn answer() -> i32: 42\n";
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.answer as CallableFunction)(), 42);
});

test("host providers import only methods reached by resolved HIR calls", async () => {
  const source = `pub trait Device:
    fn first(self) -> i32
    fn second(self) -> i32
pub fn read() -> i32 $ Device:
    $.use(Device).first()
`;
  const compilation = compileToWat(source, { hostCapabilities: ["Device"] });
  const device = compilation.hir.traits.find((trait) => trait.name === "Device")!;
  const called = calledTraitMethods(reachableProgram(compilation.hir));
  assert.deepEqual([...called], [traitMethodKey(device.index, 0)]);
  assert.match(compilation.wat, new RegExp(`"host_${device.index}_0_begin"`));
  assert.doesNotMatch(compilation.wat, new RegExp(`"host_${device.index}_1_begin"`));
  assert.doesNotMatch(
    compilation.wat,
    new RegExp(`\\(func \\$hd\\.host_trait${device.index}_method1`),
  );
  assert.doesNotMatch(compilation.wat, new RegExp(`\\$tsig${device.index}_1`));
  assert.doesNotMatch(compilation.wat, new RegExp(`\\$trait${device.index}m1`));

  const invoked: string[] = [];
  const { instance } = await instantiate(source, {
    hostCapabilities: ["Device"],
    hostSuspensionInvoke: (call) => {
      invoked.push(call.methodName);
      return { pending: false, value: 42 };
    },
  });
  assert.equal((instance.exports.read as CallableFunction)({}), 42);
  assert.deepEqual(invoked, ["first"]);

  const second = compileToWat(source.replace(".first()", ".second()"), {
    hostCapabilities: ["Device"],
  });
  assert.doesNotMatch(second.wat, new RegExp(`"host_${device.index}_0_begin"`));
  assert.match(second.wat, new RegExp(`"host_${device.index}_1_begin"`));
});

test("trait dictionary layouts contain only dynamically called methods", async () => {
  const source = `pub trait Device:
    fn first(self) -> i32
    fn second(self) -> i32
data Box: pass
impl Device for Box:
    fn first(self) -> i32: 1
    fn second(self) -> i32: 42
fn main() -> i32:
    let device: Device = Box {}
    device.second()
`;
  const { instance, compilation } = await instantiate(source);
  const device = compilation.hir.traits.find((trait) => trait.name === "Device")!;
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.doesNotMatch(compilation.wat, new RegExp(`\\$tsig${device.index}_0`));
  assert.doesNotMatch(compilation.wat, new RegExp(`\\$trait${device.index}m0`));
  assert.doesNotMatch(compilation.wat, new RegExp(`\\$tadapt0_0`));
  assert.match(compilation.wat, new RegExp(`\\$tsig${device.index}_1`));
  assert.match(compilation.wat, new RegExp(`\\$trait${device.index}m1`));
  assert.match(compilation.wat, new RegExp(`\\$tadapt0_1`));
});

test("an uncalled default trait method adds no WAT bytes", () => {
  const source = (extra: string): string => `pub trait Device:
    fn read(self) -> i32${extra}
data Box: pass
impl Device for Box:
    fn read(self) -> i32: 42
fn main() -> i32:
    let device: Device = Box {}
    device.read()
`;
  const baseline = compileToWat(source("")).wat;
  const withDefault = compileToWat(source("\n    fn unused(self) -> i32: 99")).wat;
  assert.equal(Buffer.byteLength(withDefault), Buffer.byteLength(baseline));
  assert.equal(withDefault, baseline);
});

test("a live default method adds trait calls reached from its body", async () => {
  const source = `pub trait Device:
    fn value(self) -> i32
    fn through(self, other: Device) -> i32:
        other.value()
data Box: pass
impl Device for Box:
    fn value(self) -> i32: 42
fn main() -> i32:
    let device: Device = Box {}
    device.through(device)
`;
  const { instance, compilation } = await instantiate(source);
  const device = compilation.hir.traits.find((trait) => trait.name === "Device")!;
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, new RegExp(`\\$trait${device.index}m0`));
  assert.match(compilation.wat, new RegExp(`\\$trait${device.index}m1`));
});

test("host Result errors materialize a payloadless singleton enum", async () => {
  const source = `pub enum Fault:
    Closed
pub trait Device:
    fn read(self) -> Result[i32, Fault]
pub fn recover() -> i32 $ Device:
    match $.use(Device).read():
        .Ok(value) => value
        .Err(Fault.Closed) => 42
`;
  const compilation = compileToWat(source, { hostCapabilities: ["Device"] });
  const fault = compilation.hir.enums.find((item) => item.name === "Fault")!;
  assert.match(compilation.wat, new RegExp(`global\\.get \\$e${fault.index}v0`));
  const events: ReplayEvent[] = [];
  const recorded = await instantiate(source, {
    hostCapabilities: ["Device"],
    hostSuspensionInvoke: () => ({ pending: false, value: { tag: "err" } }),
    record: (event) => events.push(event),
  });
  assert.equal((recorded.instance.exports.recover as CallableFunction)({}), 42);
  const replayed = await instantiate(source, {
    compilation: recorded.compilation,
    hostCapabilities: ["Device"],
    replay: JSON.parse(JSON.stringify(events)) as ReplayEvent[],
  });
  assert.equal((replayed.instance.exports.recover as CallableFunction)({}), 42);
  replayed.replay.assertComplete();

  const ambiguousSource = `pub enum Fault:
    Closed
    Busy
pub trait Device:
    fn read(self) -> Result[i32, Fault]
pub fn recover() -> i32 $ Device:
    match $.use(Device).read():
        .Ok(value) => value
        .Err(_) => 42
`;
  const ambiguous = await instantiate(ambiguousSource, {
    hostCapabilities: ["Device"],
    hostSuspensionInvoke: () => ({ pending: false, value: { tag: "err" } }),
  });
  assert.throws(
    () => (ambiguous.instance.exports.recover as CallableFunction)({}),
    (error: unknown) => error instanceof RuntimePanicError && error.code === "host-contract",
  );
});

test("for-loop iterator methods are declaration references, not ordinary calls", async () => {
  const source = `fn main() -> i32:
    let total = 0
    for value in 0..7:
        total = total + value
    total * 2
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
