// The conformance runtime profiles and scenarios (spec/conformance/README.md,
// Fixture Environments): the host capabilities a fixture may require, and
// the drivers `hd test --scenario` runs instead of the test cases.

import type { RUNTIME_PROFILE_NAMES, RUNTIME_SCENARIO_NAMES } from "../cli-args.ts";
import type { HostSuspensionCall, HostSuspensionOutcome } from "../compiler.ts";

export type RuntimeScenario = (typeof RUNTIME_SCENARIO_NAMES)[number];
type RuntimeProfileName = (typeof RUNTIME_PROFILE_NAMES)[number];

interface RuntimeProfile {
  readonly hostCapabilities: readonly string[];
  readonly invoke?: (call: HostSuspensionCall) => HostSuspensionOutcome;
  readonly pending?: (call: HostSuspensionCall) => boolean;
}

function pendingGate(call: HostSuspensionCall): boolean {
  return call.providerKey === "Gate" && call.methodName === "wait";
}

const pendingWrites = new WeakSet<HostSuspensionCall>();

function pendingWrite(call: HostSuspensionCall): boolean {
  if (call.providerKey !== "Console" || call.methodName !== "write_line") return false;
  if (pendingWrites.has(call)) return false;
  pendingWrites.add(call);
  return true;
}

/**
 * The `pending-first-poll` scenario (spec/conformance/README.md, Runtime
 * Scenarios): every suspending host call is pending on its first poll and
 * completes on the next.
 */
export function pendingFirstPoll(call: HostSuspensionCall): boolean {
  if (!call.suspending || pendingFirstPolls.has(call)) return false;
  pendingFirstPolls.add(call);
  return true;
}

const pendingFirstPolls = new WeakSet<HostSuspensionCall>();

function invokeMisbehavingHost(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "Gauge" || call.methodName !== "level")
    throw new Error(`misbehaving-host cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: 300 };
}

const specialFloatReads = new WeakMap<object, number>();

function invokeSpecialFloatHost(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "Sensor" || call.methodName !== "reading")
    throw new Error(`special-float-host cannot invoke ${call.providerKey}.${call.methodName}`);
  if ((typeof call.provider !== "object" && typeof call.provider !== "function") || !call.provider)
    throw new Error("special-float-host requires an object provider");
  const provider = call.provider as object;
  const count = specialFloatReads.get(provider) ?? 0;
  specialFloatReads.set(provider, count + 1);
  return { pending: false, value: count === 0 ? Number.NaN : count === 1 ? Infinity : -0 };
}

function invokeCounter(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "Counter" || call.methodName !== "add")
    throw new Error(`ready-counter cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: Number(call.arguments[0]) + Number(call.arguments[1]) };
}

function invokeFloat(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "FloatCell" || call.methodName !== "sample")
    throw new Error(`ready-float cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: call.arguments[0]! };
}

function invokeText(call: HostSuspensionCall): HostSuspensionOutcome {
  if (call.providerKey !== "TextBridge" || call.methodName !== "join")
    throw new Error(`ready-text cannot invoke ${call.providerKey}.${call.methodName}`);
  return { pending: false, value: `${call.arguments[0]}${call.arguments[1]}` };
}

export const RUNTIME_PROFILES: Readonly<Record<RuntimeProfileName, RuntimeProfile>> = {
  "misbehaving-host": { hostCapabilities: ["Gauge"], invoke: invokeMisbehavingHost },
  "pending-gate": { hostCapabilities: ["Gate"], pending: pendingGate },
  "pending-write": { hostCapabilities: [], pending: pendingWrite },
  "ready-counter": { hostCapabilities: ["Counter"], invoke: invokeCounter },
  "ready-float": { hostCapabilities: ["FloatCell"], invoke: invokeFloat },
  "ready-gate": { hostCapabilities: ["Gate"] },
  "ready-text": { hostCapabilities: ["TextBridge"], invoke: invokeText },
  "special-float-host": { hostCapabilities: ["Sensor"], invoke: invokeSpecialFloatHost },
};

export function exportedFunction(instance: WebAssembly.Instance, name: string): CallableFunction {
  const value = instance.exports[name];
  if (typeof value !== "function") throw new Error(`program has no ${name} runtime export`);
  return value;
}

export function runRuntimeScenario(
  scenario: RuntimeScenario,
  instance: WebAssembly.Instance,
  providers: readonly unknown[],
): void {
  const start = exportedFunction(instance, "__hd_start");
  const poll = exportedFunction(instance, "__hd_poll");
  start(...providers);
  if (scenario === "cancellation-cleanup") {
    if (poll() !== 0) throw new Error("cancellation-cleanup scenario did not suspend");
    exportedFunction(instance, "__hd_cancel")();
    if (exportedFunction(instance, "cleanup_ran")() !== 1)
      throw new Error("cancellation-cleanup scenario did not run cleanup");
    return;
  }
  if (scenario === "competing-drivers") {
    if (poll() !== 0) throw new Error("competing-drivers scenario did not suspend");
    exportedFunction(instance, "main")();
  } else {
    poll();
  }
  throw new Error(`${scenario} scenario completed without a runtime panic`);
}
