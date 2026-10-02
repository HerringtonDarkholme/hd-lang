// Assembles generated WAT into a Wasm binary with pinned Binaryen. Binaryen
// takes about 200 ms to load, so it is imported on the first assembly, never
// by a command that stops at WAT (compiler.ts `compileToWat`).

type Binaryen = typeof import("binaryen").default;

export interface WasmArtifact {
  readonly wat: string;
  readonly bytes: Uint8Array<ArrayBuffer>;
}

class WasmValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "WasmValidationError";
  }
}

let binaryen: Promise<Binaryen> | undefined;

function loadBinaryen(): Promise<Binaryen> {
  binaryen ??= import("binaryen").then((module) => module.default);
  return binaryen;
}

// Saturating float-to-integer casts (spec/lang/04-type-system.md#r-types.cast.saturate)
// use the non-trapping `trunc_sat` instructions.
function wasmFeatures(binaryen: Binaryen): number {
  return (
    binaryen.Features.MutableGlobals |
    binaryen.Features.ReferenceTypes |
    binaryen.Features.GC |
    binaryen.Features.NontrappingFPToInt
  );
}

export async function assembleWat(wat: string): Promise<WasmArtifact> {
  const binaryen = await loadBinaryen();
  let module: InstanceType<Binaryen["Module"]>;
  try {
    module = binaryen.parseText(wat);
  } catch (error) {
    const message = binaryenErrorMessage(error);
    // Quote the offending generated line, which the message locates.
    const line = /^(\d+):\d+:/.exec(message)?.[1];
    const text = line === undefined ? undefined : wat.split("\n")[Number(line) - 1]?.trim();
    throw new WasmValidationError(
      `Binaryen could not parse generated WAT: ${message}${text ? `\n  ${text}` : ""}`,
    );
  }

  try {
    module.setFeatures(wasmFeatures(binaryen));
    // The two validations cost under 3% of an assembly, which parsing the
    // WAT dominates. Binaryen's catches an emitter bug, Node's an encoder bug.
    if (!module.validate()) {
      throw new WasmValidationError("Binaryen rejected generated Wasm");
    }
    // `emitBinary` already returns a fresh copy of Binaryen's output.
    const bytes = module.emitBinary() as Uint8Array<ArrayBuffer>;
    if (!WebAssembly.validate(bytes)) {
      throw new WasmValidationError("Node rejected Binaryen's Wasm binary");
    }
    return { wat, bytes };
  } finally {
    module.dispose();
  }
}

function binaryenErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error)
    return String((error as { readonly message: unknown }).message);
  return String(error);
}
