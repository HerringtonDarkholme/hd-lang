// The default profile's providers that need only web-platform APIs
// (spec/cli/command-line.md#r-cli.host.default-profile): `Clock` and
// `Random`. The CLI answers them here (commands/default-profile.ts), and so
// does the playground, which runs in a browser worker with no Node APIs.

import type { HostBoundaryValue, HostSuspensionCall, HostSuspensionOutcome } from "./compiler.ts";
import { RuntimePanicError } from "./runtime-panic.ts";

/** The traits this file answers, by module and name. */
export const WEB_HOST_TRAITS: readonly { readonly module: string; readonly name: string }[] = [
  { module: "std.time", name: "Clock" },
  { module: "std.random", name: "Random" },
];

/**
 * Waits `milliseconds` in real time, blocking the thread, as a CLI program
 * may. A browser without cross-origin isolation has no `SharedArrayBuffer`
 * to wait on, so its worker spins instead.
 */
export function wait(milliseconds: number): void {
  if (milliseconds <= 0) return;
  if (typeof SharedArrayBuffer === "function") {
    try {
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds);
      return;
    } catch {
      // A thread that may not block, such as a page's main thread.
    }
  }
  const end = performance.now() + milliseconds;
  while (performance.now() < end);
}

/** `count` bytes from the platform's cryptographic random source. */
function randomBytes(count: number): Uint8Array {
  const bytes = new Uint8Array(count);
  // `getRandomValues` fills at most 65,536 bytes a call.
  for (let offset = 0; offset < count; offset += 65_536)
    crypto.getRandomValues(bytes.subarray(offset, offset + 65_536));
  return bytes;
}

/** Each answer, by the method's qualified name. */
export const WEB_HOST_ANSWERS: Readonly<
  Record<string, (call: HostSuspensionCall) => HostBoundaryValue | void>
> = {
  // The wall clock, and a monotonic clock whose origin is the start of the
  // process or worker (spec/std/time.md#r-std-time.instant.decl).
  "std.time.Clock.now": () => ({ millis: BigInt(Date.now()) }) as HostBoundaryValue,
  "std.time.Clock.monotonic": () =>
    ({ millis: BigInt(Math.floor(performance.now())) }) as HostBoundaryValue,
  "std.time.Clock.sleep": (call) => {
    const milliseconds = (call.arguments[0] as { readonly millis: bigint }).millis;
    // A negative duration panics on every provider (std-time.clock.sleep.negative).
    if (milliseconds < 0n)
      throw new RuntimePanicError(
        "explicit-panic",
        `sleep! with a negative duration of ${milliseconds} milliseconds`,
      );
    wait(Number(milliseconds));
  },
  // The platform's cryptographic random source.
  "std.random.Random.next_u64": () => new DataView(randomBytes(8).buffer).getBigUint64(0),
  "std.random.Random.fill": (call) => [...randomBytes(Number(call.arguments[0]))],
};

/** The answer to `call`, or undefined when `call` is on no trait of `WEB_HOST_TRAITS`. */
export function webHostAnswer(call: HostSuspensionCall): HostSuspensionOutcome | undefined {
  const answer = call.standardName
    ? WEB_HOST_ANSWERS[`${call.standardName}.${call.methodName}`]
    : undefined;
  if (!answer) return undefined;
  const value = answer(call);
  return value === undefined ? { pending: false } : { pending: false, value };
}
