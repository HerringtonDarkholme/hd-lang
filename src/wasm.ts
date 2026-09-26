import binaryen from "binaryen";

export const WASM_FEATURES =
  binaryen.Features.MutableGlobals | binaryen.Features.ReferenceTypes | binaryen.Features.GC;

export interface WasmArtifact {
  readonly wat: string;
  readonly bytes: Uint8Array<ArrayBuffer>;
}

export class WasmValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "WasmValidationError";
  }
}

export function assembleWat(wat: string): WasmArtifact {
  let module: binaryen.Module;
  try {
    module = binaryen.parseText(wat);
  } catch (error) {
    throw new WasmValidationError(
      `Binaryen could not parse generated WAT: ${binaryenErrorMessage(error)}`,
    );
  }

  try {
    module.setFeatures(WASM_FEATURES);
    if (!module.validate()) {
      throw new WasmValidationError("Binaryen rejected generated Wasm");
    }
    const bytes = Uint8Array.from(module.emitBinary());
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
