// Assembles generated WAT into a Wasm binary with pinned Binaryen. Binaryen
// takes about 200 ms to load, so it is imported on the first assembly, never
// by a command that stops at WAT (compiler.ts `compileToWat`).

type Binaryen = typeof import("binaryen").default;

export interface WasmArtifact {
  readonly wat: string;
  readonly bytes: Uint8Array<ArrayBuffer>;
  /** Where the WAT's panic sites landed in `bytes` (emitter/panic-sites.ts), if it has any. */
  readonly siteMap?: SiteMap;
}

/**
 * The code offsets of `bytes` where a panic site starts, ascending, and the
 * site index each starts, or -1 where code with no site starts.
 */
export interface SiteMap {
  readonly offsets: readonly number[];
  readonly sites: readonly number[];
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
    // `emitBinary` already returns a fresh copy of Binaryen's output. Panic
    // sites need its source map, which Binaryen writes only with a URL; the
    // URL's custom section is dropped again, so the module stays the same.
    let bytes: Uint8Array<ArrayBuffer>;
    let siteMap: SiteMap | undefined;
    if (wat.includes(";;@ s")) {
      const emitted = module.emitBinary(SOURCE_MAP_URL);
      bytes = withoutCustomSection(emitted.binary, "sourceMappingURL");
      siteMap = decodeSiteMap(emitted.sourceMap ?? "");
    } else bytes = module.emitBinary() as Uint8Array<ArrayBuffer>;
    if (!WebAssembly.validate(bytes)) {
      throw new WasmValidationError("Node rejected Binaryen's Wasm binary");
    }
    return siteMap ? { wat, bytes, siteMap } : { wat, bytes };
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

const SOURCE_MAP_URL = "hd.map";

/** An unsigned LEB128 number of `bytes` at `at`, and the offset after it. */
function readLeb(bytes: Uint8Array, at: number): readonly [value: number, next: number] {
  let value = 0;
  let scale = 1;
  let offset = at;
  while (true) {
    const byte = bytes[offset++]!;
    value += (byte & 0x7f) * scale;
    if (byte < 0x80) return [value, offset];
    scale *= 128;
  }
}

/** `binary` without its custom section `name`. */
function withoutCustomSection(binary: Uint8Array, name: string): Uint8Array<ArrayBuffer> {
  const decoder = new TextDecoder();
  // The 8-byte preamble, then sections: an id byte, a size, and the body.
  let offset = 8;
  while (offset < binary.length) {
    const [size, body] = readLeb(binary, offset + 1);
    const end = body + size;
    if (binary[offset] === 0) {
      const [length, text] = readLeb(binary, body);
      if (decoder.decode(binary.subarray(text, text + length)) === name) {
        const result = new Uint8Array(binary.length - (end - offset));
        result.set(binary.subarray(0, offset));
        result.set(binary.subarray(end), offset);
        return result;
      }
    }
    offset = end;
  }
  return new Uint8Array(binary);
}

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/**
 * The site map of Binaryen's source map: one line of segments, whose column
 * is a byte offset, and whose source, which the emitter names `sN`, is site
 * N. A segment of one field starts code with no site.
 */
function decodeSiteMap(sourceMap: string): SiteMap {
  const parsed = JSON.parse(sourceMap || "{}") as {
    readonly sources?: readonly string[];
    readonly mappings?: string;
  };
  const names = (parsed.sources ?? []).map((source) => Number(source.slice(1)));
  const offsets: number[] = [];
  const sites: number[] = [];
  let offset = 0;
  let source = 0;
  for (const segment of (parsed.mappings ?? "").split(",")) {
    if (segment === "") continue;
    // Base64 VLQ: five bits a digit, low digit first; bit 0 is the sign.
    const fields: number[] = [];
    let value = 0;
    let scale = 1;
    for (const character of segment) {
      const digit = BASE64.indexOf(character);
      value += (digit & 31) * scale;
      if (digit & 32) scale *= 32;
      else {
        fields.push(value % 2 === 1 ? -(value - 1) / 2 : value / 2);
        value = 0;
        scale = 1;
      }
    }
    offset += fields[0]!;
    if (fields.length > 1) source += fields[1]!;
    offsets.push(offset);
    sites.push(fields.length > 1 ? (names[source] ?? -1) : -1);
  }
  return { offsets, sites };
}

/** The panic site of the code at byte `offset` of the module, if any. */
export function siteAt(map: SiteMap, offset: number): number | undefined {
  let low = 0;
  let high = map.offsets.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (map.offsets[middle]! <= offset) {
      found = middle;
      low = middle + 1;
    } else high = middle - 1;
  }
  const site = found < 0 ? -1 : map.sites[found]!;
  return site < 0 ? undefined : site;
}
