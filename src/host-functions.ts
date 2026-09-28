// The host side of the generic host-function boundary (src/README.md,
// Compiler/library boundary). A `lib/std` function with an
// `@intrinsic("name")` line that is not a runtime primitive
// (emitter/intrinsics.ts) is imported as `host:<name>` and runs the entry
// here. Arguments and results are boundary values: numbers for the scalar
// types (`bool` as 0 or 1, `char` as its code point, `i64` as a BigInt) and
// JavaScript strings for `string`. Adding a host-backed std function needs
// its `lib/std` declaration and one entry here, nothing in the compiler.

import { UnsupportedAtRunTimeError } from "./runtime-panic.ts";

export type HostFunctionValue = number | bigint | string;

export type HostFunction = (...arguments_: HostFunctionValue[]) => HostFunctionValue | void;

export const HOST_FUNCTIONS: Readonly<Record<string, HostFunction>> = {
  // Unicode Default Case Conversion with full mappings and no locale
  // (spec/10-modules.md#string-methods).
  string_lower: (text) => String(text).toLowerCase(),
  string_upper: (text) => String(text).toUpperCase(),
  // `println` panics when its `write_line!` stays pending or returns `.Err`
  // (spec/10-modules.md#console). The spec leaves the panic category open
  // (MHP-1), so the run stops with a prototype code.
  println_pending: () => printlnPanic("its write_line! call is pending on a host operation"),
  println_error: () => printlnPanic("write_line! returned .Err(ConsoleError)"),
};

function printlnPanic(cause: string): never {
  throw new UnsupportedAtRunTimeError(
    "unsupported-println-panic",
    `println panics because ${cause}; the spec leaves this panic's category open (MHP-1)`,
  );
}
