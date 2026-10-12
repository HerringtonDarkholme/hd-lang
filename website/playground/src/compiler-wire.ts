// The request and response of the playground's compiler wasm, the four
// exports of crates/hd_web (playground-wasm.md §3.1). A request is the file
// count, each file's path and text, the entry path, then one goal byte;
// counts and lengths are unsigned LEB128 and strings UTF-8
// (crates/hd_web/src/wire.rs). The response is JSON text plus the program's
// bytes; the wat goal compiles as run and also answers the program's
// WebAssembly text, output kind 2. The playground's Rust runner and the Node smoke test
// (scripts/test-web-compiler.js) share this module.

/** The goal byte of a request. */
export const GOAL = { run: 0, check: 1, test: 2, wat: 3 } as const;
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

/**
 * One program of the test goal: the unit-test program (`file` null), or an
 * integration test file's; its bytes follow the earlier programs'.
 */
export interface WireProgram {
  readonly file: string | null;
  readonly size: number;
}

/** One test case of the test goal, in the order `hd test` reports them. */
export interface WireTest {
  /** The index of its program. */
  readonly program: number;
  readonly name: string;
  readonly file: string;
  readonly line: number;
  /** `it`, `it_each`, `it_prop` or `it_prop_with`. */
  readonly kind: string;
  readonly ignore: string | null;
  readonly expectPanic: string | null;
  /** Why the case cannot run yet. */
  readonly unsupported: string | null;
  /** A property case's base seed, as `hd test` gives it. */
  readonly seed: number | null;
  /** Its `hd.test.i` and `hd.init.j` export numbers; null when it cannot run. */
  readonly run: readonly [number, number] | null;
}

/**
 * A compiled request answers `diagnostics`, and for the test goal its
 * `programs` and `tests`; a malformed one answers `error`.
 */
export interface WireResponse {
  readonly diagnostics?: readonly WireDiagnostic[];
  readonly programs?: readonly WireProgram[];
  readonly tests?: readonly WireTest[];
  readonly error?: string;
}

export interface Compiled {
  readonly status: number;
  readonly json: WireResponse;
  /** The programs one after another; empty when the compile produced none. */
  readonly wasm: Uint8Array;
  /** The wat goal's text of the program; empty for every other goal. */
  readonly wat: string;
}

/** The bytes of each of the test goal's programs, in order. */
export function programBytes(compiled: Compiled): Uint8Array[] {
  let at = 0;
  return (compiled.json.programs ?? []).map(({ size }) => {
    const bytes = compiled.wasm.subarray(at, at + size);
    at += size;
    return bytes;
  });
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
    wat: new TextDecoder().decode(output(2)),
  };
}
