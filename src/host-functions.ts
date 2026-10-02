// The host side of the generic host-function boundary (src/README.md,
// Compiler/library boundary). A `lib/std` function with an
// `@intrinsic("name")` line that is not a runtime primitive
// (emitter/intrinsics.ts) is imported as `host:<name>` and runs the entry
// here. Arguments and results are boundary values: numbers for the scalar
// types (`bool` as 0 or 1, `char` as its code point, `i64` as a BigInt) and
// JavaScript strings for `string`. Adding a host-backed std function needs
// its `lib/std` declaration and one entry here, nothing in the compiler.

import type { HostSuspensionCall, HostSuspensionOutcome } from "./compiler.ts";
import { propertyRun } from "./property-tests.ts";
import { RuntimePanicError, runtimePanicCategory } from "./runtime-panic.ts";

type HostFunctionValue = number | bigint | string;

export type HostFunction = (...arguments_: HostFunctionValue[]) => HostFunctionValue | void;

export const HOST_FUNCTIONS: Readonly<Record<string, HostFunction>> = {
  // Unicode Default Case Conversion with full mappings and no locale
  // (spec/std/text.md#r-std-text.string.lower).
  string_lower: (text) => String(text).toLowerCase(),
  string_upper: (text) => String(text).toUpperCase(),
  // A float's `Display` text (spec/lang/04-type-system.md#numeric-display).
  format_f64: (value) => displayF64(Number(value)),
  format_f32: (value) => displayF32(Number(value)),
  // The one panic primitive of `lib/std` (spec/std/README.md#standard-library-primitives):
  // a checked runtime panic of a stable category that shows `message`, such
  // as a failed `assert`'s `assertion-failed`
  // (spec/lang/10-modules.md#r-module.testing.assert-equal-debug). An empty
  // message shows none, as for `index-out-of-bounds`.
  panic: (category, message) => {
    throw new RuntimePanicError(
      runtimePanicCategory(String(category)),
      String(message) === "" ? undefined : String(message),
    );
  },
  // `Choices` outside `hd test` draws at random; the test runner replaces
  // these with its recording draws (src/property-tests.ts).
  ...propertyRun().hostFunctions,
};

/**
 * An f32 shows the shortest decimal that rounds back to the same f32, in the
 * f64 notation (spec/lang/04-type-system.md#numeric-display).
 */
function displayF32(value: number): string {
  if (!Number.isFinite(value)) return displayF64(value);
  for (let digits = 1; digits <= 9; digits += 1) {
    const shortest = Number(value.toPrecision(digits));
    if (Math.fround(shortest) === value) return displayF64(Object.is(value, -0) ? -0 : shortest);
  }
  return displayF64(value);
}

function displayF64(value: number): string {
  if (Number.isNaN(value)) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  if (Object.is(value, -0)) return "-0.0";
  const rendered = value.toString();
  return !rendered.includes(".") && !rendered.includes("e") ? `${rendered}.0` : rendered;
}

/** What a built-in host provider may use from the embedder. */
interface HostProviderContext {
  readonly console?: (text: string, provider: unknown) => void;
}

/**
 * Built-in implementations of host capability methods, keyed
 * `Trait.method`. Each call arrives through the generic per-method bridge
 * (emitter/host-providers.ts) like any host capability's.
 */
export const HOST_PROVIDERS: Readonly<
  Record<string, (call: HostSuspensionCall, host: HostProviderContext) => HostSuspensionOutcome>
> = {
  // The host console (spec/lang/10-modules.md#console): `write_line!` writes its
  // line when first polled and is then ready with `.Ok(())`. The host reports
  // no write failure, so it never builds a `ConsoleError`.
  "Console.write_line": (call, host) => {
    host.console?.(String(call.arguments[0]), call.provider);
    return { pending: false, value: { tag: "ok" } };
  },
};

/**
 * Host capabilities whose calls are neither recorded nor replayed: `hd
 * replay` writes console lines again rather than reading them back. Whether
 * a replay should capture them is an owner question
 * (future-work/OPEN_ISSUES.md, Mutable Host Providers).
 */
export const UNRECORDED_PROVIDERS: ReadonlySet<string> = new Set(["Console"]);
