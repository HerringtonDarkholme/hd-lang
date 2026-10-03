import assert from "node:assert/strict";
import test from "node:test";

import { standardPublicNames } from "./standard-library.ts";
import { STANDARD_MODULES } from "./standard-sources.ts";

for (const [module, name] of [
  ["cli", "Cli"],
  ["host", "MapArgs"],
  ["fs", "MemoryFs"],
  ["path", "Path"],
  ["json", "Json"],
  ["encoding", "hex_encode"],
  ["digest", "sha256"],
  ["regex", "Regex"],
] as const) {
  test(`std.${module} is registered and exposes its actual declarations`, () => {
    assert.ok(STANDARD_MODULES.includes(module));
    assert.ok(standardPublicNames(module)?.includes(name));
  });
}
