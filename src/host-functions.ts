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
import { RuntimePanicError } from "./runtime-panic.ts";

export type HostFunctionValue = number | bigint | string;

export type HostFunction = (...arguments_: HostFunctionValue[]) => HostFunctionValue | void;

export const HOST_FUNCTIONS: Readonly<Record<string, HostFunction>> = {
  // Unicode Default Case Conversion with full mappings and no locale
  // (spec/std/text.md#r-std-text.string.lower).
  string_lower: (text) => String(text).toLowerCase(),
  string_upper: (text) => String(text).toUpperCase(),
  // The one-scalar string of a Unicode scalar value, for `\u{...}` in
  // `std.text.process_escapes` (spec/05-expressions.md#prefixed-strings).
  string_from_scalar: (point) => String.fromCodePoint(Number(point)),
  // A failed `assert_equal` (lib/std/testing.hd) panics with its message
  // (spec/10-modules.md#r-module.testing.assert-equal-debug).
  assertion_failed: (message) => {
    throw new RuntimePanicError("assertion-failed", String(message));
  },
  // `Choices` outside `hd test` draws at random; the test runner replaces
  // these with its recording draws (src/property-tests.ts).
  ...propertyRun().hostFunctions,
};

/** What a built-in host provider may use from the embedder. */
export interface HostProviderContext {
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
  // The host console (spec/10-modules.md#console): `write_line!` writes its
  // line when first polled and is then ready with `.Ok()`. The host reports
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
