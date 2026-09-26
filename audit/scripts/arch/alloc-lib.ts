// Shared helpers for the phase 5.1-5.3 architecture probes.
// Compiles through the public compiler API, never edits src/, and instruments
// emitted WAT by wrapping every allocation instruction with a per-type counter.
import { readFileSync } from "node:fs";
import { analyze, instantiate } from "../../../src/compiler.ts";
import { emitWat } from "../../../src/emitter/index.ts";
import { assembleWat } from "../../../src/wasm.ts";

export const COMMIT = "bd985d7";
export const ROOT = new URL("../../../", import.meta.url).pathname;

export function header(command: string): string {
  return `<!-- commit ${COMMIT}; command: ${command}; date: ${new Date().toISOString()} -->\n`;
}

export function watFor(source: string): string {
  const analysis = analyze(source);
  if (!analysis.hir) {
    const text = analysis.diagnostics
      .map((d) => `${d.code}: ${d.message}`)
      .join("\n");
    throw new Error(`probe failed to check:\n${text}`);
  }
  return emitWat(analysis.hir);
}

export function readProbe(path: string): string {
  return readFileSync(path.startsWith("/") ? path : ROOT + path, "utf8");
}

/** Index just past the form that opens at `start` (which must be '('). */
export function formEnd(text: string, start: number): number {
  let depth = 0;
  for (let index = start; index < text.length; index += 1) {
    const char = text[index];
    if (char === '"') {
      index += 1;
      while (index < text.length && text[index] !== '"') {
        if (text[index] === "\\") index += 1;
        index += 1;
      }
    } else if (char === ";" && text[index + 1] === ";") {
      while (index < text.length && text[index] !== "\n") index += 1;
    } else if (char === "(") depth += 1;
    else if (char === ")") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  throw new Error(`unbalanced form at ${start}`);
}

export interface TopForm {
  readonly kind: string;
  readonly name: string;
  readonly text: string;
  readonly start: number;
  readonly end: number;
}

/** Top-level forms inside `(module ...)`, with `rec` groups kept whole. */
export function topForms(wat: string): TopForm[] {
  const open = wat.indexOf("(module");
  const forms: TopForm[] = [];
  let index = open + "(module".length;
  const moduleEnd = formEnd(wat, open) - 1;
  while (index < moduleEnd) {
    const next = wat.indexOf("(", index);
    if (next < 0 || next >= moduleEnd) break;
    const end = formEnd(wat, next);
    const text = wat.slice(next, end);
    const match = /^\((\S+)\s*(\$[^\s()]+)?/.exec(text);
    forms.push({
      kind: match?.[1] ?? "?",
      name: match?.[2] ?? "",
      text,
      start: next,
      end,
    });
    index = end;
  }
  return forms;
}

const ALLOCATION =
  /\((struct\.new(?:_default)?|array\.new(?:_fixed|_default|_data|_elem)?)\s+(\$[^\s()]+)/g;

export interface AllocationSite {
  readonly op: string;
  readonly type: string;
  readonly func: string;
}

export function allocationSites(wat: string): AllocationSite[] {
  const sites: AllocationSite[] = [];
  for (const form of topForms(wat)) {
    const func =
      form.kind === "func" ? form.name : `<${form.kind} ${form.name}>`;
    for (const match of form.text.matchAll(ALLOCATION))
      sites.push({ op: match[1]!, type: match[2]!, func });
  }
  return sites;
}

export const isRuntimeFunction = (name: string): boolean =>
  name.startsWith("$hd.");

function counterName(type: string): string {
  return `$audit.${type.slice(1)}`;
}

function wrapAllocations(text: string, types: Set<string>): string {
  let output = "";
  let cursor = 0;
  ALLOCATION.lastIndex = 0;
  for (;;) {
    ALLOCATION.lastIndex = cursor;
    const match = ALLOCATION.exec(text);
    if (!match) break;
    const start = match.index;
    const end = formEnd(text, start);
    const type = match[2]!;
    types.add(type);
    const head = match[0];
    const inner = wrapAllocations(text.slice(start + head.length, end), types);
    const counter = counterName(type);
    output +=
      text.slice(cursor, start) +
      `(block (result (ref ${type})) (global.set ${counter} (i32.add (global.get ${counter}) (i32.const 1))) ${head}${inner})`;
    cursor = end;
  }
  return output + text.slice(cursor);
}

export interface Instrumented {
  readonly wat: string;
  readonly types: readonly string[];
}

/** Wrap each allocation inside function bodies with a per-type counter bump. */
export function instrument(wat: string): Instrumented {
  const types = new Set<string>();
  let output = "";
  let cursor = 0;
  for (const form of topForms(wat)) {
    if (form.kind !== "func") continue;
    output += wat.slice(cursor, form.start) + wrapAllocations(form.text, types);
    cursor = form.end;
  }
  output += wat.slice(cursor);
  const closing = output.lastIndexOf(")");
  const globals = [...types]
    .map(
      (type) =>
        `  (global ${counterName(type)} (export "audit:${type.slice(1)}") (mut i32) (i32.const 0))`,
    )
    .join("\n");
  return {
    wat: `${output.slice(0, closing)}\n${globals}\n)`,
    types: [...types],
  };
}

export interface CountedInstance {
  readonly instance: WebAssembly.Instance;
  readonly wat: string;
  counts(): Record<string, number>;
  total(): number;
  reset(): void;
}

/**
 * Instantiate `source` through the compiler's own `instantiate` (same host
 * imports), swapping in the instrumented binary at the WebAssembly boundary.
 */
export async function instantiateCounted(
  source: string,
): Promise<CountedInstance> {
  const { wat, types } = instrument(watFor(source));
  const bytes = assembleWat(wat).bytes;
  const original = WebAssembly.instantiate;
  const replacement = ((_bytes: unknown, imports?: WebAssembly.Imports) =>
    original(bytes, imports)) as typeof WebAssembly.instantiate;
  WebAssembly.instantiate = replacement;
  let instance: WebAssembly.Instance;
  try {
    ({ instance } = await instantiate(source));
  } finally {
    WebAssembly.instantiate = original;
  }
  const global = (type: string): WebAssembly.Global =>
    instance.exports[`audit:${type.slice(1)}`] as WebAssembly.Global;
  return {
    instance,
    wat,
    counts() {
      const result: Record<string, number> = {};
      for (const type of types) {
        const value = global(type).value as number;
        if (value !== 0) result[type] = value;
      }
      return result;
    },
    total() {
      return types.reduce(
        (sum, type) => sum + (global(type).value as number),
        0,
      );
    },
    reset() {
      for (const type of types) global(type).value = 0;
    },
  };
}

export async function instantiatePlain(
  source: string,
): Promise<WebAssembly.Instance> {
  return (await instantiate(source)).instance;
}

export function callExport(
  instance: WebAssembly.Instance,
  name: string,
  ...args: number[]
): number {
  const entry = instance.exports[name];
  if (typeof entry !== "function") throw new Error(`no export ${name}`);
  return (entry as (...values: number[]) => number)(...args);
}

export function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? sorted[middle]!
    : (sorted[middle - 1]! + sorted[middle]!) / 2;
}

/** Median wall time in ms of `runs` calls after `warmup` calls. */
export function timeCall(run: () => unknown, runs = 7, warmup = 2): number {
  for (let index = 0; index < warmup; index += 1) run();
  const samples: number[] = [];
  for (let index = 0; index < runs; index += 1) {
    const start = performance.now();
    run();
    samples.push(performance.now() - start);
  }
  return median(samples);
}

/** WAT excerpt: the type rec group, globals, and every non-runtime function. */
export function excerpt(
  wat: string,
  include: (form: TopForm) => boolean = () => true,
): string {
  const parts: string[] = [];
  for (const form of topForms(wat)) {
    if (form.kind === "rec") {
      const lines = form.text
        .split("\n")
        .filter((line) => !/^\s*$/.test(line))
        .join("\n");
      parts.push(lines);
    } else if (form.kind === "global" && !form.name.startsWith("$hd."))
      parts.push(form.text);
    else if (
      form.kind === "func" &&
      !isRuntimeFunction(form.name) &&
      include(form)
    )
      parts.push(form.text);
  }
  return parts.join("\n\n");
}
