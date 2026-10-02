import { readFileSync } from "node:fs";
import { withIntrinsicMethods } from "./intrinsic-methods.ts";

// The toy standard library: one hd source file per `std` module, in the
// top-level `lib/std/` directory, with a submodule in a subdirectory:
// `std.testing.arbitrary` is `lib/std/testing/arbitrary.hd`.
// `standard-library.ts` joins what a program uses into the one module the
// prototype compiles. The browser playground embeds these files through its
// `node:fs` shim, as it does for the emitter's runtime `.wat` files.

/** The `std` modules that the loader joins, by module path (`std.<name>`). */
export const STANDARD_MODULES = [
  "annotation",
  "cmp",
  "format",
  "function",
  "hash",
  "collections",
  "console",
  "iter",
  "num",
  "ops",
  "option",
  "process",
  "resource",
  "result",
  "task",
  "testing",
  "testing.arbitrary",
  "text",
  "time",
] as const;

export type StandardModule = (typeof STANDARD_MODULES)[number];

/**
 * `std` modules whose hd declarations a checker pass adds itself, rather
 * than the loader: `std.structure` (checker/typed-derivation.ts) and
 * `std.inspect` (checker/standard-traits.ts).
 */
export type CompilerModule = "structure" | "inspect";

const sources = new Map<string, string>();

function fileSource(name: StandardModule | CompilerModule): string {
  const path = name.replaceAll(".", "/");
  return readFileSync(new URL(`../../lib/std/${path}.hd`, import.meta.url), "utf8");
}

/**
 * The hd source of `std.<name>`, with its operation intrinsics expanded
 * (checker/intrinsic-methods.ts).
 */
export function standardSource(name: StandardModule | CompilerModule): string {
  let source = sources.get(name);
  if (source === undefined) {
    source = withIntrinsicMethods(fileSource(name), () => fileSource("num"));
    sources.set(name, source);
  }
  return source;
}
