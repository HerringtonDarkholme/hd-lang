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

test("host structural results expose only public data fields", () => {
  const source = `pub data Hidden:
    value: i32

pub trait Source:
    fn read(self) -> Hidden
`;
  assert.equal(
    analyze(source, { hostCapabilities: ["Source"] }).diagnostics.at(-1)?.code,
    "unsupported-host-provider-signature",
  );
});
