// Shared helpers for the 5.4-5.7 architecture probes and direction benchmarks.
// Imports compiler modules read-only; never modifies src/.
import binaryen from "binaryen";

import { check, type CheckOptions } from "../../../src/checker/index.ts";
import { instantiate, type InstantiateOptions } from "../../../src/compiler.ts";
import { emitWat } from "../../../src/emitter/index.ts";
import { parse } from "../../../src/parser/index.ts";
import { assembleWat, WASM_FEATURES } from "../../../src/wasm.ts";

export const COMMIT = "bd985d7";

export interface PhaseTimes {
  readonly parse: number;
  readonly check: number;
  readonly emit: number;
  readonly assemble: number;
  readonly total: number;
}

export interface PhasedCompilation {
  readonly bytes: Uint8Array<ArrayBuffer>;
  readonly times: PhaseTimes;
  readonly wat: string;
}

export function compilePhased(source: string, options: CheckOptions = {}): PhasedCompilation {
  const t0 = performance.now();
  const parsed = parse(source);
  const t1 = performance.now();
  if (!parsed.program) throw new Error(`parse failed: ${JSON.stringify(parsed.diagnostics)}`);
  const checked = check(parsed.program, options);
  const t2 = performance.now();
  if (!checked.program) throw new Error(`check failed: ${JSON.stringify(checked.diagnostics)}`);
  const wat = emitWat(checked.program);
  const t3 = performance.now();
  const artifact = assembleWat(wat);
  const t4 = performance.now();
  return {
    bytes: artifact.bytes,
    times: { parse: t1 - t0, check: t2 - t1, emit: t3 - t2, assemble: t4 - t3, total: t4 - t0 },
    wat,
  };
}

// Takes the emitted WAT: binaryen.readBinary cannot re-read these GC rec groups
// without GC enabled at read time, so parse the text exactly as src/wasm.ts does.
export function optimize(wat: string, level = 2): Uint8Array<ArrayBuffer> {
  const module = binaryen.parseText(wat);
  try {
    module.setFeatures(WASM_FEATURES);
    binaryen.setOptimizeLevel(level);
    binaryen.setShrinkLevel(0);
    module.optimize();
    if (!module.validate()) throw new Error("optimized module failed validation");
    return Uint8Array.from(module.emitBinary());
  } finally {
    module.dispose();
  }
}

export function optimizedText(wat: string, level = 2): string {
  const module = binaryen.parseText(wat);
  try {
    module.setFeatures(WASM_FEATURES);
    binaryen.setOptimizeLevel(level);
    binaryen.setShrinkLevel(0);
    module.optimize();
    return module.emitText();
  } finally {
    module.dispose();
  }
}

// Instantiate through the compiler's own host-import construction, but swap the
// module bytes (for example with a binaryen -O2 build of the same program).
export async function instantiateWithBytes(
  source: string,
  bytes: Uint8Array | undefined,
  options: InstantiateOptions = {},
): Promise<WebAssembly.Instance> {
  if (!bytes) return (await instantiate(source, options)).instance;
  const original = WebAssembly.instantiate;
  const patched = (_: unknown, imports?: WebAssembly.Imports) =>
    original(bytes as BufferSource, imports);
  (WebAssembly as { instantiate: unknown }).instantiate = patched;
  try {
    return (await instantiate(source, options)).instance;
  } finally {
    (WebAssembly as { instantiate: unknown }).instantiate = original;
  }
}

export function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle]! : (sorted[middle - 1]! + sorted[middle]!) / 2;
}

export function spread(values: readonly number[]): string {
  const sorted = [...values].sort((a, b) => a - b);
  return `${sorted[0]!.toFixed(2)}..${sorted.at(-1)!.toFixed(2)}`;
}

export function timeCalls(run: () => unknown, samples: number, warmup = 2): number[] {
  for (let index = 0; index < warmup; index += 1) run();
  const times: number[] = [];
  for (let index = 0; index < samples; index += 1) {
    const start = performance.now();
    run();
    times.push(performance.now() - start);
  }
  return times;
}

export function exported(instance: WebAssembly.Instance, name: string): CallableFunction {
  const value = instance.exports[name];
  if (typeof value !== "function") throw new Error(`missing export ${name}`);
  return value;
}

export const ALLOCATION_PATTERN = /\((struct\.new(?:_default)?|array\.new(?:_fixed|_default|_data)?)\b/g;

// Split WAT into functions and count allocation instructions in each.
export function functionBodies(wat: string): Map<string, string> {
  const bodies = new Map<string, string>();
  const lines = wat.split("\n");
  let name: string | undefined;
  let buffer: string[] = [];
  for (const line of lines) {
    const match = /^\s*\(func (\$[^\s)]+)/.exec(line);
    if (match) {
      if (name) bodies.set(name, buffer.join("\n"));
      name = match[1];
      buffer = [line];
    } else if (name) buffer.push(line);
  }
  if (name) bodies.set(name, buffer.join("\n"));
  return bodies;
}

export function countMatches(text: string, pattern: RegExp): number {
  return [...text.matchAll(new RegExp(pattern.source, "g"))].length;
}

export function header(command: string): string {
  return `<!-- commit ${COMMIT}; command: ${command}; date: ${new Date().toISOString()} -->\n`;
}
