import { createHash } from "node:crypto";

import type { Diagnostic } from "./diagnostics.ts";
import { DiagnosticError } from "./diagnostics.ts";
import { check } from "./checker.ts";
import { emitWat } from "./emitter.ts";
import type { HirProgram } from "./hir.ts";
import { parse } from "./parser.ts";
import { assembleWat, type WasmArtifact } from "./wasm.ts";

export interface Compilation extends WasmArtifact {
  readonly hir: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface Analysis {
  readonly hir?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export type SuspensionTraceEvent = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

export interface ReplayEvent {
  readonly siteId: string;
  readonly functionName: string;
  readonly functionCodeId: string;
  readonly functionIndex: number;
  readonly providerKey: "$runtime";
  readonly operation: "poll";
  readonly encodedArguments: readonly [number];
  readonly encodedResult: "pending" | "ready";
  readonly providerConfigurationId: string;
}

export interface ReplaySession {
  readonly consumed: number;
  assertComplete(): void;
}

export interface InstantiateOptions {
  readonly console?: (text: string, provider: unknown) => void;
  readonly trace?: (functionIndex: number, event: SuspensionTraceEvent) => void;
  readonly pending?: (functionIndex: number, pollCount: number) => boolean;
  readonly record?: (event: ReplayEvent) => void;
  readonly replay?: readonly ReplayEvent[];
  readonly providerConfigurationId?: string;
}

export function analyze(source: string): Analysis {
  const parsed = parse(source);
  if (!parsed.program) return { diagnostics: parsed.diagnostics };
  const checked = check(parsed.program);
  return { hir: checked.program, diagnostics: checked.diagnostics };
}

export function compile(source: string): Compilation {
  const analysis = analyze(source);
  if (!analysis.hir) throw new DiagnosticError(analysis.diagnostics);
  const artifact = assembleWat(emitWat(analysis.hir));
  return { ...artifact, hir: analysis.hir, diagnostics: analysis.diagnostics };
}

export async function instantiate(source: string, options: InstantiateOptions = {}): Promise<{ compilation: Compilation; instance: WebAssembly.Instance; replay: ReplaySession }> {
  const compilation = compile(source);
  const functionNames = new Map(compilation.hir.functions.map((declaration) => [declaration.index, declaration.name]));
  const functionCodeIds = new Map(compilation.hir.functions.map((declaration) => [
    declaration.index,
    createHash("sha256")
      .update(source.slice(declaration.span.start.offset, declaration.span.end.offset))
      .digest("hex")
      .slice(0, 16),
  ]));
  const configurationId = options.providerConfigurationId ?? "default";
  const consoleBytes: number[] = [];
  const consoleByte = (provider: unknown, byte: number): void => {
    if (byte !== -1) {
      consoleBytes.push(byte);
      return;
    }
    const text = new TextDecoder().decode(Uint8Array.from(consoleBytes));
    consoleBytes.length = 0;
    options.console?.(text, provider);
  };
  let replayIndex = 0;
  const pending = (functionIndex: number, pollCount: number): number => {
    const functionName = functionNames.get(functionIndex) ?? `function#${functionIndex}`;
    const functionCodeId = functionCodeIds.get(functionIndex) ?? "unknown";
    const siteId = `${functionName}:poll:${pollCount}`;
    const expected = options.replay?.[replayIndex];
    if (options.replay) {
      if (!expected) throw new Error(`replay exhausted before suspension site ${siteId}`);
      if (expected.siteId !== siteId
        || expected.functionName !== functionName
        || expected.functionCodeId !== functionCodeId
        || expected.providerKey !== "$runtime"
        || expected.operation !== "poll"
        || expected.encodedArguments[0] !== pollCount) {
        const reason = expected.functionCodeId !== functionCodeId
          ? `function code identity '${functionCodeId}'`
          : `suspension site ${siteId}`;
        throw new Error(`replay event ${replayIndex} does not match ${reason}`);
      }
      if (expected.providerConfigurationId !== configurationId) {
        throw new Error(`replay provider configuration '${expected.providerConfigurationId}' does not match '${configurationId}'`);
      }
      replayIndex += 1;
      return expected.encodedResult === "pending" ? 1 : 0;
    }
    const isPending = options.pending?.(functionIndex, pollCount) ?? false;
    options.record?.({
      siteId,
      functionName,
      functionCodeId,
      functionIndex,
      providerKey: "$runtime",
      operation: "poll",
      encodedArguments: [pollCount],
      encodedResult: isPending ? "pending" : "ready",
      providerConfigurationId: configurationId,
    });
    return isPending ? 1 : 0;
  };
  const { instance } = await WebAssembly.instantiate(compilation.bytes, {
    hd: {
      trace: options.trace ?? (() => undefined),
      pending,
      pow_f64: Math.pow,
      console_byte: consoleByte,
    },
  });
  const replay: ReplaySession = {
    get consumed() { return replayIndex; },
    assertComplete() {
      if (options.replay && replayIndex !== options.replay.length) {
        throw new Error(`replay has ${options.replay.length - replayIndex} unconsumed event(s)`);
      }
    },
  };
  return { compilation, instance, replay };
}
