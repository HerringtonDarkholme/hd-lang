import assert from "node:assert/strict";
import test from "node:test";

import {
  buildToolchainGate,
  runToolchainGate,
  TOOLCHAIN_GATE_WAT,
} from "../src/toolchain-gate.ts";

test("the toolchain gate contains each required Wasm GC operation", () => {
  for (const operation of [
    "struct.new",
    "array.new_fixed",
    "ref.cast",
    "externref",
    "global $retained",
  ]) {
    assert.ok(TOOLCHAIN_GATE_WAT.includes(operation));
  }
  assert.ok(buildToolchainGate().bytes.length > 0);
});

test("Binaryen output executes and retains a GC reference", async () => {
  await runToolchainGate(2_000);
});
