import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate, type ReplayEvent } from "../src/compiler.ts";

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
  assert.equal(
    analyze("fn work!() -> i32: 42\nfn main() -> i32: work!()\n").diagnostics[0]?.code,
    "bang-call-outside-suspension",
  );
  assert.equal(
    analyze("fn plain() -> i32: 42\nfn main!() -> i32: plain!()\n").diagnostics[0]?.code,
    "not-suspending",
  );
  assert.equal(
    analyze("fn work!() -> i32: 42\nfn main!() -> i32:\n    pending := work()\n    pending!()\n")
      .diagnostics[0]?.code,
    "mutable-receiver-required",
  );
});

test("unresolved standard task combinators have a dedicated boundary diagnostic", () => {
  const source = `fn ready!() -> i32: 42
fn main!() -> i32: all!(ready(), ready())
`;
  assert.equal(analyze(source).diagnostics[0]?.code, "unsupported-task-combinator");
  assert.equal(
    analyze(source.replace("all!", "race!")).diagnostics[0]?.code,
    "unsupported-task-combinator",
  );

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
  assert.throws(
    () => (second.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
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
  assert.throws(
    () => (execution.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );

  const readonly =
    "fn ready!() -> i32: 42\nfn main() -> void:\n    pending := ready()\n    pending.cancel()\n";
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
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(events, [
    [1, 0],
    [1, 1],
    [0, 0],
    [0, 1],
    [0, 2],
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
    [0, 1],
    [0, 6],
    [0, 1],
    [0, 6],
    [0, 1],
    [0, 2],
  ]);
});

test("development drivers reject competing and reentrant suspension control", async () => {
  const source = `fn main!() -> i32: 42
`;

  const competing = await instantiate(source, { pending: () => true });
  (competing.instance.exports.__hd_start as CallableFunction)();
  assert.equal((competing.instance.exports.__hd_poll as CallableFunction)(), 0);
  assert.throws(
    () => (competing.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
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
    [1, 0],
    [1, 1],
    [0, 0],
    [0, 1],
    [0, 6],
    [1, 6],
    [1, 1],
    [0, 1],
    [0, 2],
    [1, 2],
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
    [1, 0],
    [1, 1],
    [0, 0],
    [0, 1],
    [0, 6],
    [1, 6],
    [1, 3],
    [0, 3],
    [1, 7],
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
    [3, 3],
    [2, 3],
    [1, 3],
    [0, 3],
    [1, 7],
    [2, 7],
    [3, 7],
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
  assert.deepEqual(
    polls.filter(([functionIndex]) => functionIndex === 0),
    [
      [0, 1],
      [0, 2],
      [0, 1],
      [0, 2],
    ],
  );
  assert.deepEqual(
    polls.filter(([functionIndex]) => functionIndex === 1),
    [[1, 1]],
  );
  assert.deepEqual(
    polls.filter(([functionIndex]) => functionIndex === 2),
    [
      [2, 1],
      [2, 2],
      [2, 3],
    ],
  );
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
  assert.deepEqual(
    polls.filter(([functionIndex]) => functionIndex === 0),
    [
      [0, 1],
      [0, 2],
      [0, 1],
      [0, 2],
      [0, 1],
      [0, 2],
    ],
  );
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
    [1, 0],
    [1, 1],
    [0, 0],
    [0, 1],
    [0, 6],
    [1, 6],
    [1, 3],
    [0, 3],
    [1, 7],
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
  assert.deepEqual(
    events.map((event) => event.siteId),
    ["main:poll:1", "child:poll:1", "main:poll:2", "child:poll:2"],
  );
  assert.equal(events[1]?.encodedResult, "pending");

  const replayed = await instantiate(source, {
    pending: () => {
      throw new Error("live pending callback must not run during replay");
    },
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.equal((replayed.instance.exports.main as CallableFunction)(), 42);
  replayed.replay.assertComplete();
  assert.equal(replayed.replay.consumed, events.length);

  const mismatched = await instantiate(source, {
    replay: events,
    providerConfigurationId: "fixture-b",
  });
  assert.throws(
    () => (mismatched.instance.exports.main as CallableFunction)(),
    /provider configuration/,
  );

  const reordered = await instantiate(`fn unused() -> i32: 0\n${source}`, {
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.equal((reordered.instance.exports.main as CallableFunction)(), 42);
  reordered.replay.assertComplete();
  assert.notEqual(
    reordered.compilation.hir.functions.find((declaration) => declaration.name === "child")?.index,
    events[1]?.functionIndex,
  );

  const changed = await instantiate(
    source.replace("fn child!() -> i32: 2", "fn child!() -> i32: 3"),
    {
      replay: events,
      providerConfigurationId: "fixture-a",
    },
  );
  assert.throws(
    () => (changed.instance.exports.main as CallableFunction)(),
    /function code identity/,
  );
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

  assert.ok(
    analyze(`fn main() -> void:
    println("missing")
`).diagnostics.some((diagnostic) => diagnostic.code === "missing-requirement"),
  );
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
  assert.throws(
    () => (nestedExecution.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );

  const forbidden = analyze(`use std.task.block_on
fn ready!() -> i32: 42
fn main() -> void:
    let pending: mut Suspend[i32] = ready()
    defer:
        _ := block_on(pending)
`);
  assert.ok(
    forbidden.diagnostics.some((diagnostic) => diagnostic.code === "suspension-forbidden-context"),
  );
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
  assert.throws(
    () => (failure.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );

  const unsupported = analyze(`use std.testing.assert_equal
data Value: pass
fn main() -> void: assert_equal(Value {}, Value {}, reason="not derived")
`);
  assert.ok(unsupported.diagnostics.some((diagnostic) => diagnostic.code === "missing-partial-eq"));
});

test("module bindings lower to Wasm globals shared with declared functions", async () => {
  const source = `let count: i32 = 0
label := "requests"

fn increment() -> void:
    count = count + 1

fn value() -> i32:
    count

increment()
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(
    compilation.hir.globals.map((global) => [global.name, global.type, global.mutable]),
    [
      ["count", "i32", true],
      ["label", "string", false],
    ],
  );
  assert.match(compilation.wat, /\(global \$g0 \(mut i32\) \(i32\.const 0\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), undefined);
  assert.equal((instance.exports.value as CallableFunction)(), 1);

  const immutable = analyze(`answer := 41
fn change() -> void: answer = 42
`);
  assert.ok(
    immutable.diagnostics.some((diagnostic) => diagnostic.code === "non-reassignable-binding"),
  );
});
