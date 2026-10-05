import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate, type HostBoundaryValue, type ReplayEvent } from "../src/compiler.ts";
import { RuntimePanicError } from "../src/runtime-panic.ts";

const SOURCE = `pub data Case:
    pub example: u32?
    pub seed: i64
    pub size: i32
    pub replay: List[i64]

pub trait Cases:
    fn start(self) -> Case

let observed: i32 = 0

pub fn main() -> void $ Cases:
    case := $.use(Cases).start()
    example := match case.example:
        .Some(value) => value
        .None => u32(0)
    observed = i32(case.seed) + case.size + i32(example) + i32(case.replay[0])

pub fn read() -> i32:
    observed
`;

test("structural host results cross the live and replay boundaries", async () => {
  const events: ReplayEvent[] = [];
  const recorded = await instantiate(SOURCE, {
    hostCapabilities: ["Cases"],
    hostSuspensionInvoke: () => ({
      pending: false,
      value: {
        example: { tag: "some", value: 2 },
        seed: 7n,
        size: 3,
        replay: [11n],
      },
    }),
    providerConfigurationId: "structural-result",
    record: (event) => events.push(event),
  });
  (recorded.instance.exports.main as CallableFunction)({});
  assert.equal((recorded.instance.exports.read as CallableFunction)(), 23);
  assert.equal(events.length, 1);

  const replayed = await instantiate(SOURCE, {
    compilation: recorded.compilation,
    hostCapabilities: ["Cases"],
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    providerConfigurationId: "structural-result",
    replay: JSON.parse(JSON.stringify(events)) as ReplayEvent[],
  });
  (replayed.instance.exports.main as CallableFunction)({});
  assert.equal((replayed.instance.exports.read as CallableFunction)(), 23);
  replayed.replay.assertComplete();
});

test("structural host results reject malformed nested values", async () => {
  const valid = {
    example: { tag: "none" },
    seed: 7n,
    size: 3,
    replay: [11n],
  } as const;
  const cyclic: Record<string, HostBoundaryValue> = { ...valid };
  cyclic.example = cyclic;
  const malformed: readonly HostBoundaryValue[] = [
    { ...valid, replay: [2n ** 63n] },
    { ...valid, example: { tag: "some" } },
    { ...valid, replay: 11n },
    { example: { tag: "none" }, seed: 7n, size: 3 },
    { ...valid, extra: 0 },
    cyclic,
  ];
  for (const value of malformed) {
    const runtime = await instantiate(SOURCE, {
      hostCapabilities: ["Cases"],
      hostSuspensionInvoke: () => ({ pending: false, value }),
    });
    assert.throws(
      () => (runtime.instance.exports.main as CallableFunction)({}),
      (error: unknown) => error instanceof RuntimePanicError && error.code === "host-contract",
    );
  }
});

// spec/lang/10-modules.md#private-fields-at-a-boundary: a private field
// crosses only with the consent its direction needs.
test("host structural results expose private data fields only with consent", () => {
  const source = `pub data Hidden:
    value: i32

pub trait Source:
    fn read(self) -> Hidden
`;
  assert.equal(
    analyze(source, { hostCapabilities: ["Source"] }).diagnostics.at(-1)?.code,
    "boundary-private-field",
  );
  const consented = `use std.serde.Deserialize

@derive(Deserialize)
pub data Hidden:
    value: i32

pub trait Source:
    fn read(self) -> Hidden
`;
  assert.deepEqual(
    analyze(consented, { hostCapabilities: ["Source"] }).diagnostics.map(({ code }) => code),
    [],
  );
});

test("a host argument with a private field needs Serialize, not Deserialize", () => {
  const source = `use std.serde.Deserialize

@derive(Deserialize)
pub data Hidden:
    value: i32

pub trait Sink:
    fn write(self, value: Hidden) -> void
`;
  const diagnostics = analyze(source, { hostCapabilities: ["Sink"] }).diagnostics;
  assert.deepEqual(
    diagnostics.map(({ code }) => code),
    ["boundary-private-field"],
  );
  assert.match(diagnostics[0]!.message, /std\.serde\.Serialize/);
});

// `Process.run!` takes a `List[string]` and returns
// `Result[ProcessOutput, ProcessError]`, a data success and an enum error
// (spec/lang/10-modules.md#processes, module.boundary.allowed).
test("a List[string] argument, a data success, and an enum error cross the boundary", async () => {
  const source = [
    "use std.process.{Process, ProcessError}",
    "",
    "pub fn main!() -> void $ Process + Console:",
    '    for name in ["ok", "missing", "other"]:',
    '        match $.use(Process).run!(name, ["a", "b c", ""], "in"):',
    '            .Ok(out) => println("ok ${out.stdout} ${out.status}")',
    '            .Err(.NotFound) => println("not found")',
    '            .Err(.Other(message)) => println("other ${message}")',
    '            .Err(_) => println("refused")',
    "",
  ].join("\n");
  const answers: Record<string, HostBoundaryValue> = {
    ok: { tag: "ok", value: { stdout: "out", stderr: "", status: 3 } },
    missing: { tag: "err", value: { tag: "NotFound" } },
    other: { tag: "err", value: { tag: "Other", message: "boom" } },
  };
  const run = async (options: Parameters<typeof instantiate>[1]): Promise<string[]> => {
    const lines: string[] = [];
    const { instance, compilation } = await instantiate(source, {
      ...options,
      hostCapabilities: ["Process"],
      console: (text) => lines.push(text),
    });
    const main = compilation.hir.functions.find(({ entry }) => entry)!;
    (instance.exports[main.name] as CallableFunction)(
      ...main.requirements.map((requirement) => ({ requirement })),
    );
    return lines;
  };
  const events: ReplayEvent[] = [];
  const calls: unknown[] = [];
  const live = await run({
    hostSuspensionInvoke: (call) => {
      calls.push(call.arguments);
      return { pending: false, value: answers[call.arguments[0] as string]! };
    },
    record: (event) => events.push(event),
  });
  const expected = ["ok out 3", "not found", "other boom"];
  assert.deepEqual(live, expected);
  assert.deepEqual(calls[0], ["ok", ["a", "b c", ""], "in"]);
  const replayed = await run({
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    replay: JSON.parse(JSON.stringify(events)) as ReplayEvent[],
  });
  assert.deepEqual(replayed, expected);
});

// Any boundary-safe argument crosses as a value tree (module.boundary.allowed);
// a std type, as `Path` or `Duration`, crosses with its private fields, and a
// newtype as its base value.
test("structural host arguments cross the live and replay boundaries", async () => {
  const source = [
    "use std.path.Path",
    "use std.time.{Duration, ms}",
    "",
    "pub enum Shape:",
    "    Dot",
    "    Box(w: i32, h: i32)",
    "",
    "pub data Order:",
    "    pub id: u64",
    "    pub tags: List[string]",
    "    pub note: string?",
    "    pub shape: Shape",
    "    pub pair: (i32, f64)",
    "",
    "pub enum Failure:",
    "    Missing(path: Path)",
    "    Other(message: string)",
    "",
    "pub trait Sink:",
    "    fn take(self, order: Order, path: Path, wait: Duration, bytes: List[u8]) -> Result[string, Failure]",
    "",
    "pub fn main() -> void $ Sink + Console:",
    '    order := Order { id: 7, tags: ["a", "bé"], note: .Some("n"), shape: Shape.Box(+2, -3), pair: (+4, 1.5) }',
    '    for name in ["ok", "missing"]:',
    "        match $.use(Sink).take(order, Path(name), 250ms, [9, 255]):",
    '            .Ok(text) => println("ok ${text}")',
    '            .Err(.Missing(path)) => println("missing ${path}")',
    '            .Err(.Other(message)) => println("other ${message}")',
    "",
  ].join("\n");
  const run = async (options: Parameters<typeof instantiate>[1]): Promise<string[]> => {
    const lines: string[] = [];
    const { instance, compilation } = await instantiate(source, {
      ...options,
      hostCapabilities: ["Sink"],
      console: (text) => lines.push(text),
    });
    const main = compilation.hir.functions.find(({ entry }) => entry)!;
    (instance.exports[main.name] as CallableFunction)(
      ...main.requirements.map((requirement) => ({ requirement })),
    );
    return lines;
  };
  const events: ReplayEvent[] = [];
  const calls: unknown[] = [];
  const live = await run({
    hostSuspensionInvoke: (call) => {
      calls.push(call.arguments);
      const path = call.arguments[1] as string;
      return {
        pending: false,
        value:
          path === "ok"
            ? { tag: "ok", value: "done" }
            : { tag: "err", value: { tag: "Missing", path } },
      };
    },
    record: (event) => events.push(event),
  });
  const expected = ["ok done", "missing missing"];
  assert.deepEqual(live, expected);
  assert.deepEqual(calls[0], [
    {
      id: 7n,
      tags: ["a", "bé"],
      note: { tag: "some", value: "n" },
      shape: { tag: "Box", w: 2, h: -3 },
      pair: [4, 1.5],
    },
    "ok",
    { millis: 250n },
    [9, 255],
  ]);
  const replayed = await run({
    hostSuspensionInvoke: () => {
      throw new Error("live host provider must not run during replay");
    },
    replay: JSON.parse(JSON.stringify(events)) as ReplayEvent[],
  });
  assert.deepEqual(replayed, expected);
});
