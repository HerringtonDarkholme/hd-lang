import { readFileSync } from "node:fs";

import type { SourceDocument } from "../diagnostics.ts";

// The toy standard library: one hd source file per `std` module, in the
// top-level `lib/std/` directory, with a submodule in a subdirectory:
// `std.testing.arbitrary` is `lib/std/testing/arbitrary.hd`. `std.prelude`
// (`lib/std/prelude.hd`) is `use` lines only. `standard-library.ts` joins
// the modules that a program's use graph reaches into the one module the
// prototype compiles. The browser playground embeds these files through its
// `node:fs` shim, as it does for the emitter's runtime `.wat` files.

/** The `std` modules that the loader can join, by module path (`std.<name>`). */
export const STANDARD_MODULES = [
  "annotation",
  "cli",
  "cmp",
  "convert",
  "error",
  "format",
  "function",
  "hash",
  "host",
  "collections",
  "console",
  "fs",
  "path",
  "json",
  "encoding",
  "digest",
  "iter",
  "num",
  "ops",
  "option",
  "process",
  "random",
  "regex",
  "resource",
  "result",
  "task",
  "testing",
  "testing.arbitrary",
  "text",
  "time",
  "prelude",
  "prelude.testing",
] as const;

export type StandardModule = (typeof STANDARD_MODULES)[number];

/**
 * `std` modules whose hd declarations a checker pass adds itself, rather
 * than the loader: `std.structure` (checker/typed-derivation.ts) and
 * `std.inspect` (checker/standard-traits.ts).
 */
type CompilerModule = "structure" | "inspect";

const sources = new Map<string, string>();
const documents = new Map<string, SourceDocument>();

/** The physical source descriptor shared by every span from one std file. */
export function standardDocument(name: StandardModule | CompilerModule): SourceDocument {
  const cached = documents.get(name);
  if (cached) return cached;
  const path = name.replaceAll(".", "/");
  const document = { file: `lib/std/${path}.hd`, text: standardSource(name) };
  documents.set(name, document);
  return document;
}

function fileSource(name: StandardModule | CompilerModule): string {
  const path = name.replaceAll(".", "/");
  return readFileSync(new URL(`../../lib/std/${path}.hd`, import.meta.url), "utf8");
}

/** The hd source of `std.<name>`. */
export function standardSource(name: StandardModule | CompilerModule): string {
  let source = sources.get(name);
  if (source === undefined) {
    source = fileSource(name);
    sources.set(name, source);
  }
  return source;
}
