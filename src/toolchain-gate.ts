import { pathToFileURL } from "node:url";

import { assembleWat, type WasmArtifact } from "./wasm.ts";

export const TOOLCHAIN_GATE_WAT = `(module
  (type $point (struct
    (field $x (mut i32))
    (field $y i32)))
  (type $bytes (array (mut i8)))

  (import "host" "external" (global $external externref))

  (global $retained (mut (ref null $point)) (ref.null $point))

  (func (export "allocate_and_sum") (param $x i32) (result i32)
    (local $point-value (ref $point))
    (local $byte-values (ref $bytes))
    (local $opaque anyref)

    (local.set $point-value
      (struct.new $point (local.get $x) (i32.const 5)))
    (global.set $retained (local.get $point-value))
    (local.set $byte-values
      (array.new_fixed $bytes 3
        (i32.const 1)
        (i32.const 7)
        (i32.const 3)))
    (local.set $opaque (local.get $point-value))

    (i32.add
      (struct.get $point $x
        (ref.cast (ref $point) (local.get $opaque)))
      (array.get_u $bytes (local.get $byte-values) (i32.const 1))))

  (func (export "retained_x") (result i32)
    (struct.get $point $x
      (ref.as_non_null (global.get $retained))))

  (func (export "external_is_null") (result i32)
    (ref.is_null (global.get $external))))`;

interface GateExports extends WebAssembly.Exports {
  allocate_and_sum(value: number): number;
  retained_x(): number;
  external_is_null(): number;
}

export function buildToolchainGate(): WasmArtifact {
  return assembleWat(TOOLCHAIN_GATE_WAT);
}

export async function runToolchainGate(iterations = 10_000): Promise<void> {
  const artifact = buildToolchainGate();
  const external = { source: "hd-lang-toolchain-gate" };
  const { instance } = await WebAssembly.instantiate(artifact.bytes, {
    host: { external },
  } as unknown as WebAssembly.Imports);
  const exports = instance.exports as GateExports;

  if (exports.external_is_null() !== 0) {
    throw new Error("externref import was unexpectedly null");
  }
  for (let index = 0; index < iterations; index += 1) {
    const result = exports.allocate_and_sum(index);
    if (result !== index + 7) {
      throw new Error(`allocation result mismatch at ${index}: ${result}`);
    }
  }
  if (exports.retained_x() !== iterations - 1) {
    throw new Error("the mutable GC global did not retain the last frame");
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) {
  await runToolchainGate();
  console.log("Wasm GC toolchain gate passed");
}
