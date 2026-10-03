import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate, type ReplayEvent } from "../src/compiler.ts";
import { conformance, fixture } from "./fixture.ts";

test("suspending functions construct GC frames and bang calls drive them", async () => {
  const source = `trait Clock

data FixedClock: pass

impl Clock for FixedClock

fn add_two!(value: i32) -> i32 $ Clock:
    _ := $.use(Clock)
    value + 2

fn main!() -> i32:
    $.with(Clock=FixedClock {}):
        add_two!(40)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$s0 \(struct/);
  assert.match(compilation.wat, /\(func \$f0 .*\(result \(ref null \$s0\)\)/);
  assert.match(compilation.wat, /\(func \$poll0/);
  assert.match(compilation.wat, /struct\.set \$s0 \$s0state/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic suspending functions box frame arguments and unbox direct or stored results", async () => {
  const source = conformance("runtime/valid/generic-suspending-function");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic suspension frames retain trait dictionaries across child polls", async () => {
  const source = conformance("runtime/valid/generic-suspending-function-bound");
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.match(compilation.wat, /field \$s1b0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.get \$s1 \$s1b0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("ordinary suspending calls are cold values and bang calls need a driver", () => {
  const cold = analyze(conformance("typing/warnings/unused-cold-suspension"));
  assert.equal(cold.diagnostics[0]?.code, "unused-local-binding");
  assert.equal(cold.hir?.functions[1]?.locals[0]?.type, "suspend(0):i32");
  assert.equal(
    analyze(conformance("typing/invalid/bang-call-in-plain-function")).diagnostics[0]?.code,
    "bang-call-outside-suspension",
  );
  assert.equal(
    analyze(conformance("typing/invalid/bang-call-non-suspending")).diagnostics[0]?.code,
    "not-suspending",
  );
  assert.equal(
    analyze(conformance("typing/invalid/drive-readonly-suspension")).diagnostics[0]?.code,
    "mutable-receiver-required",
  );
});

// Trace events: 0 construct, 1 poll, 2 complete, 3 cancel, 6 pending. In
// both fixtures `slow!` is function 0, `fast!` 1, and `main!` 2.
test("all! polls every child in order, then only the unfinished ones", async () => {
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(fixture("suspension/05-all-task-combinator"), {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(events, [
    [2, 0],
    [2, 1],
    [0, 0],
    [1, 0],
    [0, 1],
    [0, 6],
    [1, 1],
    [1, 2],
    [2, 6],
    [2, 1],
    [0, 1],
    [0, 2],
    [2, 2],
  ]);
});

test("cancelling all! cancels only its unfinished children", async () => {
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(fixture("suspension/05-all-task-combinator"), {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex === 0,
  });
  (instance.exports.__hd_start as CallableFunction)();
  assert.equal((instance.exports.__hd_poll as CallableFunction)(), 0);
  (instance.exports.__hd_cancel as CallableFunction)();
  assert.deepEqual(events.slice(-2), [
    [2, 3],
    [0, 3],
  ]);
});

test("race! returns the first result and cancels the losers before it completes", async () => {
  const source = fixture("suspension/05-race-task-combinator");
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex === 0,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  const children = events.filter(([functionIndex]) => functionIndex < 2);
  assert.deepEqual(children, [
    [0, 0],
    [1, 0],
    [0, 1],
    [0, 6],
    [1, 1],
    [1, 2],
    [0, 3],
  ]);
  assert.deepEqual(events.at(-1), [2, 2]);

  // Cancelling a pending race! cancels every child.
  const cancelled: Array<[number, number]> = [];
  const pending = await instantiate(source, {
    trace: (functionIndex, event) => cancelled.push([functionIndex, event]),
    pending: (functionIndex) => functionIndex < 2,
  });
  (pending.instance.exports.__hd_start as CallableFunction)();
  assert.equal((pending.instance.exports.__hd_poll as CallableFunction)(), 0);
  (pending.instance.exports.__hd_cancel as CallableFunction)();
  assert.deepEqual(
    cancelled.filter(([functionIndex, event]) => functionIndex < 2 && event === 3),
    [
      [0, 3],
      [1, 3],
    ],
  );
});

test("a module may declare its own all! without importing std.task.all", () => {
  assert.deepEqual(analyze(fixture("suspension/05-user-defined-all")).diagnostics, []);
});

test("explicit mutable suspension bindings are one-shot", async () => {
  const valid = conformance("runtime/valid/stored-suspension-single-drive");
  const { instance } = await instantiate(valid);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const secondDrive = conformance("runtime/panic/second-drive-of-completed-suspension");
  const second = await instantiate(secondDrive);
  assert.throws(() => (second.instance.exports.main as CallableFunction)());
});

test("cold suspension cancellation is synchronous and idempotent", async () => {
  const source = conformance("runtime/valid/cold-suspension-cancel-idempotent");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(func \$cancel0/);
  assert.match(compilation.wat, /i32\.const 3/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("cancelled suspensions cannot be driven and readonly values cannot be cancelled", async () => {
  const cancelled = conformance("runtime/panic/drive-cancelled-suspension");
  const execution = await instantiate(cancelled);
  assert.throws(() => (execution.instance.exports.main as CallableFunction)());

  const readonly = conformance("typing/invalid/cancel-readonly-suspension");
  assert.equal(analyze(readonly).diagnostics[0]?.code, "mutable-receiver-required");
});

test("defer suites cannot suspend", () => {
  const source = conformance("typing/invalid/bang-call-in-defer");
  assert.equal(analyze(source).diagnostics[0]?.code, "suspending-defer");
});

test("suspension state transitions are observable through the trace ABI", async () => {
  const source = fixture(
    "suspension/10-suspension-state-transitions-are-observable-through-the-trace-abi",
  );
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
  const source = fixture("suspension/11-deterministic-host-pending");
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
  const source = fixture(
    "suspension/12-development-drivers-reject-competing-and-reentrant-suspension-control",
  );

  const competing = await instantiate(source, { pending: () => true });
  (competing.instance.exports.__hd_start as CallableFunction)();
  assert.equal((competing.instance.exports.__hd_poll as CallableFunction)(), 0);
  assert.throws(() => (competing.instance.exports.main as CallableFunction)());
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
  assert.ok(nestedPollError instanceof Error);

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
  assert.ok(nestedCancelError instanceof Error);
});

test("child pending propagates through the parent frame and restores locals", async () => {
  const source = conformance("runtime/valid/suspending-call-preserves-locals");
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

  const nested = conformance("runtime/valid/nested-suspending-call");
  const nestedResult = await instantiate(nested, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((nestedResult.instance.exports.main as CallableFunction)(), 42);
});

test("started-frame cancellation cancels the child and runs registered cleanup", async () => {
  const source = conformance("runtime/valid/suspending-call-with-defer");
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
  assert.throws(() => poll());
});

test("cancellation unwinds child frames before parent cleanup", async () => {
  const source = conformance("runtime/valid/cancellation-cleanup-order");
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
  const source = conformance("runtime/valid/sequential-suspending-calls");
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex < 2 && pollCount === 1,
  });
  assert.match(compilation.wat, /field \$s2child0 \(mut \(ref null \$s0\)\)/);
  assert.match(compilation.wat, /field \$s2child1 \(mut \(ref null \$s1\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves nested expression evaluation", async () => {
  const source = conformance("runtime/valid/suspending-calls-in-binary-expression");
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.doesNotMatch(compilation.wat, /unsupported-nested-suspension/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves nested argument order", async () => {
  const source = conformance("runtime/valid/suspending-calls-as-arguments");
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
  const source = conformance("runtime/valid/suspending-calls-in-branches");
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
  const source = conformance("runtime/valid/suspending-calls-in-loops");
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering preserves match bindings and suspending guards", async () => {
  const source = conformance("runtime/valid/suspending-match-guards");
  const { instance } = await instantiate(source, {
    pending: (_functionIndex, pollCount) => pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering propagates Result failures after child completion", async () => {
  const source = conformance("runtime/valid/suspending-result-propagation");
  const { instance } = await instantiate(source, {
    pending: (_functionIndex, pollCount) => pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 35);
});

test("CFG suspension cancellation cancels the active child and runs scoped cleanup", async () => {
  const source = conformance("runtime/valid/suspending-call-in-scoped-defer");
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
  assert.throws(() => poll());
});

test("CFG suspension lowering installs providers produced after resumption", async () => {
  const source = conformance("runtime/valid/provider-from-suspending-call");
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.match(compilation.wat, /struct\.new \$trait0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("CFG suspension lowering nests dynamic trait suspensions", async () => {
  const source = conformance("runtime/valid/dynamic-suspending-method");
  const { instance } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("suspension poll decisions record and replay with configuration identity", async () => {
  const source = fixture(
    "suspension/26-suspension-poll-decisions-record-and-replay-with-configuration-identity",
  );
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

  const reordered = await instantiate(
    fixture("suspension/26-replay-survives-unrelated-declaration"),
    {
      replay: events,
      providerConfigurationId: "fixture-a",
    },
  );
  assert.equal((reordered.instance.exports.main as CallableFunction)(), 42);
  reordered.replay.assertComplete();
  assert.notEqual(
    reordered.compilation.hir.functions.find((declaration) => declaration.name === "child")?.index,
    events[1]?.functionIndex,
  );

  const changed = await instantiate(fixture("suspension/26-replay-rejects-function-code-change"), {
    replay: events,
    providerConfigurationId: "fixture-a",
  });
  assert.throws(
    () => (changed.instance.exports.main as CallableFunction)(),
    /function code identity/,
  );
});

test("host provider polls record and replay", async () => {
  const source = fixture("suspension/32-host-provider-polls-record-and-replay");
  const events: ReplayEvent[] = [];
  let providerPolls = 0;
  const recorded = await instantiate(source, {
    hostCapabilities: ["Gate"],
    hostSuspensionPending: () => {
      providerPolls += 1;
      return providerPolls === 1;
    },
    providerConfigurationId: "gate-a",
    record: (event) => events.push(event),
  });
  assert.equal((recorded.instance.exports.main as CallableFunction)({ name: "gate" }), undefined);
  assert.equal((recorded.instance.exports.recorded_result as CallableFunction)(), 42);
  const providerEvents = events.filter((event) => event.operation === "provider-poll");
  assert.deepEqual(
    providerEvents.map((event) => event.encodedResult),
    ["pending", "ready"],
  );
  assert.match(providerEvents[0]!.siteId, /^wait_once:provider:Gate\.wait:/);

  const replayed = await instantiate(source, {
    hostCapabilities: ["Gate"],
    hostSuspensionPending: () => {
      throw new Error("live host provider must not run during replay");
    },
    pending: () => {
      throw new Error("live runtime poll must not run during replay");
    },
    providerConfigurationId: "gate-a",
    replay: events,
  });
  assert.equal((replayed.instance.exports.main as CallableFunction)({ name: "gate" }), undefined);
  assert.equal((replayed.instance.exports.recorded_result as CallableFunction)(), 42);
  replayed.replay.assertComplete();
});

test("host provider scalar arguments and results record and replay", async () => {
  const source = fixture("suspension/33-host-provider-scalar-arguments-and-results");
  const events: ReplayEvent[] = [];
  let providerPolls = 0;
  const recorded = await instantiate(source, {
    hostCapabilities: ["Counter"],
    hostSuspensionInvoke: ({ arguments: values }) => {
      providerPolls += 1;
      return providerPolls === 1
        ? { pending: true }
        : { pending: false, value: Number(values[0]) + Number(values[1]) };
    },
    providerConfigurationId: "counter-a",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "counter" });
  assert.equal((recorded.instance.exports.recorded_result as CallableFunction)(), 42);
  const providerEvents = events.filter((event) => event.operation === "provider-poll");
  assert.deepEqual(
    providerEvents.map(({ encodedArguments, encodedResult, encodedValue }) => ({
      encodedArguments,
      encodedResult,
      encodedValue,
    })),
    [
      {
        encodedArguments: [
          { kind: "i32", value: 20 },
          { kind: "i32", value: 22 },
        ],
        encodedResult: "pending",
        encodedValue: undefined,
      },
      {
        encodedArguments: [
          { kind: "i32", value: 20 },
          { kind: "i32", value: 22 },
        ],
        encodedResult: "ready",
        encodedValue: { kind: "i32", value: 42 },
      },
    ],
  );

  const replayed = await instantiate(source, {
    hostCapabilities: ["Counter"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    pending: () => {
      throw new Error("live runtime poll must not run during replay");
    },
    providerConfigurationId: "counter-a",
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "counter" });
  assert.equal((replayed.instance.exports.recorded_result as CallableFunction)(), 42);
  replayed.replay.assertComplete();
});

test("usize host arguments and Result payloads preserve unsigned bits during replay", async () => {
  const source = [
    "use std.testing.assert",
    "pub trait Sizes:",
    "    fn echo!(self, value: usize) -> Result[usize, usize]",
    "pub fn main!() -> void $ Sizes:",
    "    sizes := $.use(Sizes)",
    "    let maximum: usize = 4294967295",
    "    match sizes.echo!(maximum):",
    '        .Ok(value) => assert(value == maximum, reason="unsigned success")',
    '        .Err(_) => panic("unexpected error")',
    "    match sizes.echo!(0):",
    '        .Ok(_) => panic("unexpected success")',
    '        .Err(value) => assert(value == maximum, reason="unsigned error")',
    "",
  ].join("\n");
  const events: ReplayEvent[] = [];
  const recorded = await instantiate(source, {
    hostCapabilities: ["Sizes"],
    hostSuspensionInvoke: ({ arguments: values }) => ({
      pending: false,
      value: { tag: values[0] === 0 ? "err" : "ok", value: 4294967295 },
    }),
    providerConfigurationId: "unsigned-sizes",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "sizes" });
  const polls = events.filter((event) => event.operation === "provider-poll");
  assert.deepEqual(
    polls.map((event) => event.encodedArguments),
    [[{ kind: "u32", value: 4294967295 }], [{ kind: "u32", value: 0 }]],
  );
  assert.deepEqual(
    polls.map((event) => event.encodedValue),
    [
      { kind: "ok", value: { kind: "u32", value: 4294967295 } },
      { kind: "err", value: { kind: "u32", value: 4294967295 } },
    ],
  );
  const replayed = await instantiate(source, {
    hostCapabilities: ["Sizes"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "unsigned-sizes",
    replay: JSON.parse(JSON.stringify(events)) as ReplayEvent[],
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "sizes" });
  replayed.replay.assertComplete();
});

test("host provider f64 replay encoding preserves non-JSON numbers", async () => {
  const source = fixture("suspension/35-host-provider-f64-values-use-durable-bit-encoding");
  const events: ReplayEvent[] = [];
  let providerPolls = 0;
  const recorded = await instantiate(source, {
    hostCapabilities: ["FloatCell"],
    hostSuspensionInvoke: () => {
      providerPolls += 1;
      return providerPolls === 1 ? { pending: true } : { pending: false, value: -0 };
    },
    providerConfigurationId: "float-a",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "float" });
  assert.ok(Object.is((recorded.instance.exports.recorded_result as CallableFunction)(), -0));
  const providerEvents = events.filter((event) => event.operation === "provider-poll");
  assert.deepEqual(providerEvents[0]?.encodedArguments, [
    { bits: "8000000000000000", kind: "f64" },
  ]);
  assert.equal(providerEvents[1]?.encodedValue?.kind, "f64");
  assert.match(
    providerEvents[1]?.encodedValue?.kind === "f64" ? providerEvents[1].encodedValue.bits : "",
    /^[0-9a-f]{16}$/,
  );
  assert.deepEqual(JSON.parse(JSON.stringify(providerEvents)), providerEvents);

  const replayed = await instantiate(source, {
    hostCapabilities: ["FloatCell"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "float-a",
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "float" });
  assert.ok(Object.is((replayed.instance.exports.recorded_result as CallableFunction)(), -0));
  replayed.replay.assertComplete();
});

test("host provider string arguments and results use durable UTF-8 replay encoding", async () => {
  const source = fixture("suspension/36-host-provider-strings-use-utf8-boundary");
  const events: ReplayEvent[] = [];
  const calls: Array<readonly (number | bigint | string)[]> = [];
  const recorded = await instantiate(source, {
    hostCapabilities: ["TextBridge"],
    hostSuspensionInvoke: (call) => {
      calls.push(call.arguments);
      return { pending: false, value: `${call.arguments[0]}${call.arguments[1]}` };
    },
    providerConfigurationId: "text-a",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "text" });
  assert.deepEqual(calls, [["host: ", "λ🙂"]]);
  const providerEvent = events.find((event) => event.operation === "provider-poll");
  assert.deepEqual(providerEvent?.encodedArguments, [
    { kind: "string", utf8: "686f73743a20" },
    { kind: "string", utf8: "cebbf09f9982" },
  ]);
  assert.deepEqual(providerEvent?.encodedValue, {
    kind: "string",
    utf8: "686f73743a20cebbf09f9982",
  });
  assert.deepEqual(JSON.parse(JSON.stringify(events)), events);

  const replayed = await instantiate(source, {
    hostCapabilities: ["TextBridge"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "text-a",
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "text" });
  replayed.replay.assertComplete();
});

test("a Result[T, E] host result crosses the bridge and replays", async () => {
  const source = [
    "use std.testing.assert_equal",
    "",
    "pub trait Lookup:",
    "    fn find!(self, key: string) -> Result[i32, string]",
    "",
    "pub fn main!() -> void $ Lookup:",
    "    lookup := $.use(Lookup)",
    '    let found = match lookup.find!("a"):',
    "        .Ok(value) => value",
    "        .Err(_) => 0",
    '    let missing = match lookup.find!("b"):',
    "        .Ok(_) => 0",
    "        .Err(message) => i32(message.len())",
    '    assert_equal(found * 10 + missing, 44, reason="both sides cross the bridge")',
    "",
  ].join("\n");
  const events: ReplayEvent[] = [];
  const recorded = await instantiate(source, {
    hostCapabilities: ["Lookup"],
    hostSuspensionInvoke: (call) => ({
      pending: false,
      value: call.arguments[0] === "a" ? { tag: "ok", value: 4 } : { tag: "err", value: "gone" },
    }),
    providerConfigurationId: "lookup",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "lookup" });
  assert.deepEqual(
    events.flatMap((event) => (event.operation === "provider-poll" ? [event.encodedValue] : [])),
    [
      { kind: "ok", value: { kind: "i32", value: 4 } },
      { kind: "err", value: { kind: "string", utf8: "676f6e65" } },
    ],
  );
  const replayed = await instantiate(source, {
    hostCapabilities: ["Lookup"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "lookup",
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "lookup" });
  replayed.replay.assertComplete();
});

test("host console lines are neither recorded nor replayed", async () => {
  const source = 'pub fn main() -> void $ Console:\n    println("hi")\n';
  const events: ReplayEvent[] = [];
  const lines: string[] = [];
  const recorded = await instantiate(source, {
    console: (text) => lines.push(text),
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "console" });
  assert.deepEqual(events, []);
  const replayed = await instantiate(source, {
    console: (text) => lines.push(text),
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "console" });
  replayed.replay.assertComplete();
  assert.deepEqual(lines, ["hi", "hi"]);
});

test("a leading U+FEFF host string is text on the live and replayed boundary", async () => {
  const source = [
    "use std.testing.assert_equal",
    "",
    "pub trait TextBridge:",
    "    fn join!(self, left: string, right: string) -> string",
    "",
    "pub fn main!() -> void $ TextBridge:",
    '    joined := $.use(TextBridge).join!("\\u{FEFF}", "x")',
    '    assert_equal(joined.len(), 4, reason="a leading U+FEFF is three bytes of text, not a byte order mark")',
    "",
  ].join("\n");
  const events: ReplayEvent[] = [];
  const calls: Array<readonly (number | bigint | string)[]> = [];
  const recorded = await instantiate(source, {
    hostCapabilities: ["TextBridge"],
    hostSuspensionInvoke: (call) => {
      calls.push(call.arguments);
      return { pending: false, value: `${call.arguments[0]}${call.arguments[1]}` };
    },
    providerConfigurationId: "text-a",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({ name: "text" });
  assert.deepEqual(calls, [["﻿", "x"]]);
  const replayed = await instantiate(source, {
    hostCapabilities: ["TextBridge"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "text-a",
    replay: events,
  });
  (replayed.instance.exports.main as CallableFunction)({ name: "text" });
  replayed.replay.assertComplete();
});

test("println requires Console and streams displayed UTF-8 through the host boundary", async () => {
  const source = fixture(
    "suspension/27-println-requires-console-and-streams-displayed-utf-8-through-the-host-bo",
  );
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
  // The host console goes through the generic per-method bridge.
  assert.doesNotMatch(compilation.wat, /console_byte|\$hd\.console_print/);
  assert.match(compilation.wat, /_result_tag/);

  assert.ok(
    analyze(conformance("typing/invalid/println-without-console")).diagnostics.some(
      (diagnostic) => diagnostic.code === "missing-requirement",
    ),
  );
});

test("multi-provider use preserves requested tuple order through Wasm GC", async () => {
  const source = conformance("runtime/valid/multi-provider-use-order");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list 2/);
});

test("imported block_on drives stored suspensions", async () => {
  const source = conformance("runtime/valid/block-on-stored-suspension");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.drive as CallableFunction)(), 42);
  assert.equal((instance.exports.main as CallableFunction)(), undefined);
  assert.match(compilation.wat, /global \$hd\.driver-active/);

  const forbidden = analyze(conformance("typing/invalid/block-on-in-defer"));
  assert.ok(
    forbidden.diagnostics.some((diagnostic) => diagnostic.code === "suspension-forbidden-context"),
  );
});

test("imported assert_equal compares supported structural values", async () => {
  const source = conformance("runtime/valid/assert-equal-nested-tuple");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const failure = await instantiate(conformance("runtime/panic/assert-equal-lists-in-main"));
  assert.throws(() => (failure.instance.exports.main as CallableFunction)());

  const unsupported = analyze(
    conformance("typing/invalid/assert-equal-fieldless-data-without-partial-eq"),
  );
  assert.ok(unsupported.diagnostics.some((diagnostic) => diagnostic.code === "missing-eq"));
});

test("module bindings lower to Wasm globals shared with declared functions", async () => {
  const source = conformance("runtime/valid/module-binding-shared-with-functions");
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

  const immutable = analyze(conformance("typing/invalid/reassign-short-module-binding"));
  assert.ok(
    immutable.diagnostics.some((diagnostic) => diagnostic.code === "non-reassignable-binding"),
  );
});
