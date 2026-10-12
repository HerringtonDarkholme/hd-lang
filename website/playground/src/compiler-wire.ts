// The request and response of the playground's compiler wasm, the four
// exports of crates/hd_web (playground-wasm.md §3.1). A request is the file
// count, each file's path and text, the entry path, then one goal byte;
// counts and lengths are unsigned LEB128 and strings UTF-8
// (crates/hd_web/src/wire.rs). The response is JSON text plus the program's
// bytes. The playground's Rust runner and the Node smoke test
// (scripts/test-web-compiler.js) share this module.

/** The goal byte of a request. */
export const GOAL = { run: 0, check: 1, test: 2 } as const;
/** `hd_compile`'s status: compiled, compiled with errors, or a request that did not decode. */
export const STATUS = { ok: 0, errors: 1, malformed: 2 } as const;

/** The compiler wasm's exports. */
export interface CompilerExports {
  readonly memory: WebAssembly.Memory;
  hd_input(len: number): number;
  hd_compile(): number;
  hd_output(kind: number): number;
  hd_output_len(kind: number): number;
}

/** One diagnostic of the response; a field the compiler cannot place is null. */
export interface WireDiagnostic {
  readonly path: string | null;
  readonly code: string | null;
  readonly severity: "error" | "warning";
  readonly message: string;
  readonly line: number | null;
  readonly column: number | null;
  readonly endLine: number | null;
  readonly endColumn: number | null;
}

/** A compiled request answers `diagnostics`; a malformed one answers `error`. */
export interface WireResponse {
  readonly diagnostics?: readonly WireDiagnostic[];
  readonly error?: string;
}

export interface Compiled {
  readonly status: number;
  readonly json: WireResponse;
  /** Empty when the compile produced no program. */
  readonly wasm: Uint8Array;
}

function leb(out: number[], n: number): void {
  do {
    const low = n & 0x7f;
    n >>>= 7;
    out.push(n === 0 ? low : low | 0x80);
  } while (n !== 0);
}

/** The request bytes for `files` (path to text), built for `goal` from `entry`. */
export function encodeRequest(
  files: Readonly<Record<string, string>>,
  entry: string,
  goal: number,
): Uint8Array {
  const out: number[] = [];
  const encoder = new TextEncoder();
  const string = (text: string): void => {
    const bytes = encoder.encode(text);
    leb(out, bytes.length);
    for (const byte of bytes) out.push(byte);
  };
  leb(out, Object.keys(files).length);
  for (const [path, text] of Object.entries(files)) {
    string(path);
    string(text);
  }
  string(entry);
  out.push(goal);
  return Uint8Array.from(out);
}

/** Sends `request` through the exports and reads both outputs. */
export function compileRequest(exports: CompilerExports, request: Uint8Array): Compiled {
  const ptr = exports.hd_input(request.length) >>> 0;
  new Uint8Array(exports.memory.buffer, ptr, request.length).set(request);
  const status = exports.hd_compile();
  // The buffers live in the compiler's memory; copy them out before the
  // next call reuses it.
  const output = (kind: number): Uint8Array => {
    const at = exports.hd_output(kind) >>> 0;
    const n = exports.hd_output_len(kind) >>> 0;
    return new Uint8Array(exports.memory.buffer, at, n).slice();
  };
  return {
    status,
    json: JSON.parse(new TextDecoder().decode(output(0))) as WireResponse,
    wasm: output(1),
  };
}
