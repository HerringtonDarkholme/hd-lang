import { createHash } from "node:crypto";

import type { Diagnostic } from "./diagnostics.ts";
import { DiagnosticError } from "./diagnostics.ts";
import { check, type CheckOptions } from "./checker/index.ts";
import { emitWat } from "./emitter/index.ts";
import type { HirProgram, ValueType } from "./hir.ts";
import { parse } from "./parser/index.ts";
import { assembleWat, type WasmArtifact } from "./wasm.ts";
import { RuntimePanicError, runtimePanicName } from "./runtime-panic.ts";

export interface Compilation extends WasmArtifact {
  readonly hir: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface Analysis {
  readonly hir?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface CompileOptions extends CheckOptions {}

export type SuspensionTraceEvent = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

interface ReplayEventBase {
  readonly siteId: string;
  readonly functionName: string;
  readonly functionCodeId: string;
  readonly functionIndex: number;
  readonly encodedResult: "pending" | "ready";
  readonly providerConfigurationId: string;
}

export interface RuntimePollReplayEvent extends ReplayEventBase {
  readonly providerKey: "$runtime";
  readonly operation: "poll";
  readonly encodedArguments: readonly [number];
}

export interface HostPollReplayEvent extends ReplayEventBase {
  readonly providerKey: string;
  readonly operation: "provider-poll";
  readonly encodedArguments: readonly EncodedHostValue[];
  readonly encodedValue?: EncodedHostValue;
}

export interface EncodedHostInteger {
  readonly kind: "bool" | "char" | "i32";
  readonly value: number;
}

export interface EncodedHostFloat {
  readonly bits: string;
  readonly kind: "f64";
}

export interface EncodedHostString {
  readonly kind: "string";
  readonly utf8: string;
}

export type EncodedHostValue = EncodedHostFloat | EncodedHostInteger | EncodedHostString;

export type HostSuspensionValue = number | string;

export type ReplayEvent = HostPollReplayEvent | RuntimePollReplayEvent;

export interface ReplaySession {
  readonly consumed: number;
  assertComplete(): void;
}

export interface HostSuspensionCall {
  readonly arguments: readonly HostSuspensionValue[];
  readonly functionCodeId: string;
  readonly functionIndex: number;
  readonly functionName: string;
  readonly methodName: string;
  readonly provider: unknown;
  readonly providerKey: string;
  readonly siteId: string;
}

export interface HostSuspensionOutcome {
  readonly pending: boolean;
  readonly value?: HostSuspensionValue;
}

interface MutableHostSuspensionCall extends HostSuspensionCall {
  arguments: HostSuspensionValue[];
}

interface HostCallState {
  readonly argumentBytes: ReadonlyMap<number, Uint8Array>;
  readonly call: MutableHostSuspensionCall;
  outcome?: HostSuspensionOutcome;
  resultBytes?: Uint8Array;
}

interface FunctionIdentity {
  readonly codeId: string;
  readonly end: number;
  readonly index: number;
  readonly name: string;
  readonly start: number;
}

type HostImport = (...arguments_: unknown[]) => unknown;

export interface InstantiateOptions {
  readonly console?: (text: string, provider: unknown) => void;
  readonly trace?: (functionIndex: number, event: SuspensionTraceEvent) => void;
  readonly pending?: (functionIndex: number, pollCount: number) => boolean;
  readonly record?: (event: ReplayEvent) => void;
  readonly replay?: readonly ReplayEvent[];
  readonly providerConfigurationId?: string;
  readonly hostCapabilities?: readonly string[];
  readonly hostSuspensionCancel?: (call: HostSuspensionCall) => void;
  readonly hostSuspensionInvoke?: (call: HostSuspensionCall) => HostSuspensionOutcome;
  readonly hostSuspensionPending?: (call: HostSuspensionCall) => boolean;
}

export interface Instantiation {
  readonly compilation: Compilation;
  readonly instance: WebAssembly.Instance;
  readonly replay: ReplaySession;
}

function displayF64(value: number): string {
  if (Number.isNaN(value)) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  if (Object.is(value, -0)) return "-0.0";
  const rendered = value.toString();
  return !rendered.includes(".") && !rendered.includes("e") ? `${rendered}.0` : rendered;
}

function canonicalHostValue(type: ValueType, value: HostSuspensionValue): HostSuspensionValue {
  if (type === "string") {
    if (typeof value !== "string") throw new Error("host string boundary value must be a string");
    return value;
  }
  if (typeof value !== "number") throw new Error(`host ${type} boundary value must be a number`);
  if (type === "f64") return Number(value);
  if (type === "bool") return value === 0 ? 0 : 1;
  return value | 0;
}

function hexBytes(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function bytesFromHex(value: string): Uint8Array {
  if (!/^(?:[0-9a-f]{2})*$/.test(value))
    throw new Error(`replay UTF-8 bytes '${value}' are invalid`);
  return Uint8Array.from(
    Array.from({ length: value.length / 2 }, (_, index) =>
      Number.parseInt(value.slice(index * 2, index * 2 + 2), 16),
    ),
  );
}

function encodeHostValue(type: ValueType, value: HostSuspensionValue): EncodedHostValue {
  const canonical = canonicalHostValue(type, value);
  if (type === "string")
    return { kind: "string", utf8: hexBytes(new TextEncoder().encode(canonical as string)) };
  if (type !== "f64") return { kind: type as EncodedHostInteger["kind"], value: Number(canonical) };
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setFloat64(0, Number(canonical), false);
  return { bits: view.getBigUint64(0, false).toString(16).padStart(16, "0"), kind: "f64" };
}

function decodeHostValue(type: ValueType, encoded: EncodedHostValue): HostSuspensionValue {
  if (encoded.kind !== type)
    throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
  if (encoded.kind === "string")
    return new TextDecoder("utf-8", { fatal: true }).decode(bytesFromHex(encoded.utf8));
  if (encoded.kind !== "f64") return canonicalHostValue(type, encoded.value);
  if (!/^[0-9a-f]{16}$/.test(encoded.bits))
    throw new Error(`replay f64 bits '${encoded.bits}' are invalid`);
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setBigUint64(0, BigInt(`0x${encoded.bits}`), false);
  return view.getFloat64(0, false);
}

function sameEncodedHostValue(left: EncodedHostValue, right: EncodedHostValue): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "f64") return right.kind === "f64" && left.bits === right.bits;
  if (left.kind === "string") return right.kind === "string" && left.utf8 === right.utf8;
  return right.kind !== "f64" && right.kind !== "string" && left.value === right.value;
}

export function analyze(source: string, options: CompileOptions = {}): Analysis {
  const parsed = parse(source);
  if (!parsed.program) return { diagnostics: parsed.diagnostics };
  const checked = check(parsed.program, options);
  return { hir: checked.program, diagnostics: checked.diagnostics };
}

export function compile(source: string, options: CompileOptions = {}): Compilation {
  const analysis = analyze(source, options);
  if (!analysis.hir) throw new DiagnosticError(analysis.diagnostics);
  const artifact = assembleWat(emitWat(analysis.hir));
  return { ...artifact, hir: analysis.hir, diagnostics: analysis.diagnostics };
}

export async function instantiate(
  source: string,
  options: InstantiateOptions = {},
): Promise<Instantiation> {
  const compilation = compile(source, options);
  const functionIdentities: FunctionIdentity[] = [
    ...compilation.hir.functions,
    ...compilation.hir.closures,
  ].map((declaration) => ({
    codeId: createHash("sha256")
      .update(source.slice(declaration.span.start.offset, declaration.span.end.offset))
      .digest("hex")
      .slice(0, 16),
    end: declaration.span.end.offset,
    index: declaration.suspensionIndex ?? declaration.index,
    name: declaration.name,
    start: declaration.span.start.offset,
  }));
  const functionIdentity = (index: number): FunctionIdentity | undefined =>
    functionIdentities.find((identity) => identity.index === index);
  const configurationId = options.providerConfigurationId ?? "default";
  const consoleBytes: number[] = [];
  const textEncoder = new TextEncoder();
  const textDecoder = new TextDecoder("utf-8", { fatal: true });
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
    const identity = functionIdentity(functionIndex);
    const functionName = identity?.name ?? `function#${functionIndex}`;
    const functionCodeId = identity?.codeId ?? "unknown";
    const siteId = `${functionName}:poll:${pollCount}`;
    const expected = options.replay?.[replayIndex];
    if (options.replay) {
      if (!expected) throw new Error(`replay exhausted before suspension site ${siteId}`);
      if (
        expected.siteId !== siteId ||
        expected.functionName !== functionName ||
        expected.functionCodeId !== functionCodeId ||
        expected.providerKey !== "$runtime" ||
        expected.operation !== "poll" ||
        expected.encodedArguments[0] !== pollCount
      ) {
        const reason =
          expected.functionCodeId !== functionCodeId
            ? `function code identity '${functionCodeId}'`
            : `suspension site ${siteId}`;
        throw new Error(`replay event ${replayIndex} does not match ${reason}`);
      }
      if (expected.providerConfigurationId !== configurationId) {
        throw new Error(
          `replay provider configuration '${expected.providerConfigurationId}' does not match '${configurationId}'`,
        );
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
  const hostImports: Record<string, HostImport> = {};
  let stringTransformOperation = 0;
  let stringTransformInput: number[] = [];
  let stringTransformOutput: Uint8Array | undefined;
  const hostCapabilities = new Set(compilation.hir.hostCapabilities);
  const hostCall = (
    provider: unknown,
    providerKey: string,
    methodName: string,
    siteOffset: number,
    arguments_: HostSuspensionValue[],
  ): MutableHostSuspensionCall => {
    const identity = functionIdentities
      .filter(({ start, end }) => start <= siteOffset && siteOffset <= end)
      .sort((left, right) => left.end - left.start - (right.end - right.start))[0];
    if (!identity) throw new Error(`host provider call has unknown source offset ${siteOffset}`);
    const siteId = `${identity.name}:provider:${providerKey}.${methodName}:${siteOffset - identity.start}`;
    return {
      arguments: arguments_,
      functionCodeId: identity.codeId,
      functionIndex: identity.index,
      functionName: identity.name,
      methodName,
      provider,
      providerKey,
      siteId,
    };
  };
  for (const trait of compilation.hir.traits) {
    if (!hostCapabilities.has(trait.name)) continue;
    for (const method of trait.methods) {
      const prefix = `host_${trait.index}_${method.index}`;
      hostImports[`${prefix}_begin`] = (provider, siteOffset, ...arguments_) => ({
        argumentBytes: new Map(
          method.parameters.flatMap((parameter, index) =>
            parameter === "string"
              ? [[index, new Uint8Array(Number(arguments_[index]))] as const]
              : [],
          ),
        ),
        call: hostCall(
          provider,
          trait.name,
          method.name,
          siteOffset as number,
          method.parameters.map((parameter, index) =>
            parameter === "string" ? "" : canonicalHostValue(parameter, Number(arguments_[index])),
          ),
        ),
      });
      if (method.parameters.includes("string"))
        hostImports[`${prefix}_argument_byte`] = (value, argumentIndex, byteIndex, byte) => {
          const bytes = (value as HostCallState).argumentBytes.get(Number(argumentIndex));
          if (!bytes || Number(byteIndex) >= bytes.length)
            throw new Error(`host provider ${trait.name}.${method.name} received an invalid byte`);
          bytes[Number(byteIndex)] = Number(byte);
        };
      hostImports[`${prefix}_poll`] = (value) => {
        const state = value as HostCallState;
        const { call } = state;
        for (const [index, bytes] of state.argumentBytes)
          call.arguments[index] = textDecoder.decode(bytes);
        const expected = options.replay?.[replayIndex];
        if (options.replay) {
          if (!expected) throw new Error(`replay exhausted before provider site ${call.siteId}`);
          if (
            expected.operation !== "provider-poll" ||
            expected.siteId !== call.siteId ||
            expected.functionName !== call.functionName ||
            expected.functionCodeId !== call.functionCodeId ||
            expected.providerKey !== call.providerKey ||
            expected.encodedArguments.length !== call.arguments.length ||
            expected.encodedArguments.some(
              (argument, index) =>
                !sameEncodedHostValue(
                  argument,
                  encodeHostValue(method.parameters[index]!, call.arguments[index]!),
                ),
            )
          ) {
            throw new Error(
              `replay event ${replayIndex} does not match provider site ${call.siteId}`,
            );
          }
          if (expected.providerConfigurationId !== configurationId)
            throw new Error(
              `replay provider configuration '${expected.providerConfigurationId}' does not match '${configurationId}'`,
            );
          state.outcome = {
            pending: expected.encodedResult === "pending",
            ...(expected.encodedValue
              ? { value: decodeHostValue(method.result, expected.encodedValue) }
              : {}),
          };
          if (!state.outcome.pending && method.result !== "void" && !expected.encodedValue)
            throw new Error(`replay provider ${trait.name}.${method.name} has no boundary result`);
          if (!state.outcome.pending && method.result === "string")
            state.resultBytes = textEncoder.encode(state.outcome.value as string);
          replayIndex += 1;
          return state.outcome.pending ? 0 : 1;
        }
        state.outcome = options.hostSuspensionInvoke?.(call) ?? {
          pending: options.hostSuspensionPending?.(call) ?? false,
        };
        if (!state.outcome.pending && method.result !== "void" && state.outcome.value === undefined)
          throw new Error(`host provider ${trait.name}.${method.name} returned no boundary result`);
        if (state.outcome.pending) state.outcome = { pending: true };
        else if (method.result !== "void")
          state.outcome = {
            pending: false,
            value: canonicalHostValue(method.result, state.outcome.value!),
          };
        if (!state.outcome.pending && method.result === "string")
          state.resultBytes = textEncoder.encode(state.outcome.value as string);
        const event: HostPollReplayEvent = {
          siteId: call.siteId,
          functionName: call.functionName,
          functionCodeId: call.functionCodeId,
          functionIndex: call.functionIndex,
          providerKey: call.providerKey,
          operation: "provider-poll",
          encodedArguments: call.arguments.map((argument, index) =>
            encodeHostValue(method.parameters[index]!, argument),
          ),
          encodedResult: state.outcome.pending ? "pending" : "ready",
          ...(state.outcome.value === undefined
            ? {}
            : { encodedValue: encodeHostValue(method.result, state.outcome.value) }),
          providerConfigurationId: configurationId,
        };
        options.record?.(event);
        return state.outcome.pending ? 0 : 1;
      };
      hostImports[`${prefix}_cancel`] = (value) =>
        options.hostSuspensionCancel?.((value as HostCallState).call);
      if (method.result === "string") {
        hostImports[`${prefix}_result_length`] = (value) => {
          const state = value as HostCallState;
          if (!state.resultBytes)
            throw new Error(`host provider ${trait.name}.${method.name} has no ready result`);
          return state.resultBytes.length;
        };
        hostImports[`${prefix}_result_byte`] = (value, index) => {
          const state = value as HostCallState;
          if (!state.resultBytes)
            throw new Error(`host provider ${trait.name}.${method.name} has no ready result`);
          return state.resultBytes[Number(index)];
        };
      } else if (method.result !== "void") {
        hostImports[`${prefix}_result`] = (value) => {
          const state = value as HostCallState;
          if (!state.outcome || state.outcome.pending || state.outcome.value === undefined)
            throw new Error(`host provider ${trait.name}.${method.name} has no ready result`);
          return state.outcome.value;
        };
      }
    }
  }
  const { instance } = await WebAssembly.instantiate(compilation.bytes, {
    hd: {
      ...hostImports,
      trace: options.trace ?? (() => undefined),
      pending,
      pow_f64: Math.pow,
      format_f64: (value: number, index: number) => {
        const bytes = textEncoder.encode(displayF64(value));
        return index < 0 ? bytes.length : bytes[index]!;
      },
      string_transform_begin: (operation: number) => {
        stringTransformOperation = operation;
        stringTransformInput = [];
        stringTransformOutput = undefined;
      },
      string_transform_input: (byte: number) => {
        stringTransformInput.push(byte);
      },
      string_transform_output: (index: number) => {
        if (!stringTransformOutput) {
          const input = textDecoder.decode(Uint8Array.from(stringTransformInput));
          const transformed =
            stringTransformOperation === 0
              ? input.trim()
              : stringTransformOperation === 1
                ? input.toLowerCase()
                : undefined;
          if (transformed === undefined)
            throw new Error(`unknown string transform ${stringTransformOperation}`);
          stringTransformOutput = textEncoder.encode(transformed);
        }
        return index < 0 ? stringTransformOutput.length : stringTransformOutput[index]!;
      },
      panic: (code: number) => {
        throw new RuntimePanicError(runtimePanicName(code));
      },
      console_byte: consoleByte,
    },
  });
  const replay: ReplaySession = {
    get consumed() {
      return replayIndex;
    },
    assertComplete() {
      if (options.replay && replayIndex !== options.replay.length) {
        throw new Error(`replay has ${options.replay.length - replayIndex} unconsumed event(s)`);
      }
    },
  };
  return { compilation, instance, replay };
}
