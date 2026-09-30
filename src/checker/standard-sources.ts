import { readFileSync } from "node:fs";

// The toy standard library: one hd source file per `std` module, in the
// top-level `lib/std/` directory. `standard-library.ts` joins what a program
// uses into the one module the prototype compiles. The browser playground
// embeds these files through its `node:fs` shim, as it does for the emitter's
// runtime `.wat` files.

/** The `std` modules written in hd, by module path (`std.<name>`). */
export const STANDARD_MODULES = [
  "annotation",
  // `std.testing.arbitrary`, reached only through `use std.testing.arbitrary`
  // (checker/derive-arbitrary.ts).
  "arbitrary",
  "cmp",
  "format",
  "hash",
  "collections",
  "console",
  "iter",
  "num",
  "ops",
  "option",
  "process",
  "result",
  "testing",
  "text",
  "time",
] as const;

export type StandardModule = (typeof STANDARD_MODULES)[number];

const sources = new Map<string, string>();

/** The hd source of `std.<name>`. */
export function standardSource(name: StandardModule): string {
  let source = sources.get(name);
  if (source === undefined) {
    source = readFileSync(new URL(`../../lib/std/${name}.hd`, import.meta.url), "utf8");
    sources.set(name, source);
  }
  return source;
}
