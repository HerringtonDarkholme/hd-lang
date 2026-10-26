// The request and response of the playground's compiler wasm, the four
// exports of crates/hd_web (crates/hd_web/DESIGN.md §3.1). A request is the file
// count, each file's path and text, the entry path, then one goal byte and
// the goal's own fields; counts, numbers and lengths are unsigned LEB128 and
// strings UTF-8 (crates/hd_web/src/wire.rs). The response is JSON text plus
// the program's bytes; the wat goal compiles as run and also answers the
// program's WebAssembly text, output kind 2. The REPL goals build one module
// of a REPL session per request (encodeReplStart, encodeReplInput,
// encodeReplOutcome). The playground's Rust runner and the Node smoke test
// (scripts/test-web-compiler.js) share this module.

/**
 * The goal byte of a request. `replStart` starts a REPL session over the
 * files and builds its prelude, module 0; `replInput` builds the next input;
 * `replOutcome` reports how the waiting module ended.
 */
export const GOAL = {
  run: 0,
  check: 1,
  test: 2,
  wat: 3,
  replStart: 4,
  replInput: 5,
  replOutcome: 6,
  replRebuildInput: 7,
} as const;
/**
 * The outcome byte of a `replOutcome` request: the module ended with status
 * 0, so it joins the session; or it ended any other way (it did not
 * instantiate, panicked, was stopped or ended with another status), so it
 * adds nothing.
 */
export const OUTCOME = { joined: 0, dropped: 1, failed: 2 } as const;
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
 * `programs` and `tests`; a malformed one, or a REPL request that does not
 * fit the session's state, answers `error`. A REPL start or input answers
 * `input`, with the session program in the bytes when the input built; a
 * REPL outcome answers `input` and `outcome`.
 */
export interface WireResponse {
  readonly diagnostics?: readonly WireDiagnostic[];
  readonly programs?: readonly WireProgram[];
  readonly tests?: readonly WireTest[];
  /** The REPL input's number, `repl#N`'s N; 0 for a start. */
  readonly input?: number;
  /** How the input the outcome reported ended. */
  readonly outcome?: "joined" | "dropped" | "failed";
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

function string(out: number[], text: string): void {
  const bytes = new TextEncoder().encode(text);
  leb(out, bytes.length);
  for (const byte of bytes) out.push(byte);
}

/** The files, entry and goal byte that every request starts with. */
function header(files: Readonly<Record<string, string>>, entry: string, goal: number): number[] {
  const out: number[] = [];
  leb(out, Object.keys(files).length);
  for (const [path, text] of Object.entries(files)) {
    string(out, path);
    string(out, text);
  }
  string(out, entry);
  out.push(goal);
  return out;
}

/** The request bytes for `files` (path to text), built for `goal` from `entry`. */
export function encodeRequest(
  files: Readonly<Record<string, string>>,
  entry: string,
  goal: number,
): Uint8Array {
  return Uint8Array.from(header(files, entry, goal));
}

/** Starts a REPL session whose package is `files` (`src/lib.hd` when present). */
export function encodeReplStart(files: Readonly<Record<string, string>>): Uint8Array {
  return Uint8Array.from(header(files, "", GOAL.replStart));
}

/**
 * Builds `text` as the REPL session's next input: input `number`, which
 * must be greater than every earlier module's, or else one after the last
 * module. The playground numbers its inputs itself, so a rebuild keeps them.
 */
export function encodeReplInput(text: string, number?: number): Uint8Array {
  const out = header({}, "", GOAL.replInput);
  string(out, text);
  if (number !== undefined) leb(out, number);
  return Uint8Array.from(out);
}

/** Build a whole initialized-base candidate for historical replay. */
export function encodeReplRebuildInput(text: string, number: number): Uint8Array {
  const out = header({}, "", GOAL.replRebuildInput);
  string(out, text);
  leb(out, number);
  return Uint8Array.from(out);
}

/**
 * Reports how input `input` ended: `OUTCOME.joined` after status 0,
 * `OUTCOME.failed` after a panic or another status (it stays for replays,
 * hidden from later inputs), else `OUTCOME.dropped` (it did not run, or it
 * was stopped or cancelled).
 */
export function encodeReplOutcome(input: number, outcome: number): Uint8Array {
  const out = header({}, "", GOAL.replOutcome);
  leb(out, input);
  out.push(outcome);
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
