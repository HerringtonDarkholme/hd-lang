import { createHash } from "node:crypto";

import type { Diagnostic } from "./diagnostics.ts";
import { DiagnosticError, physicalSpan, sourceDocument } from "./diagnostics.ts";
import { check, type CheckOptions } from "./checker/index.ts";
import { emitWat, isRuntimePrimitive } from "./emitter/index.ts";
import {
  HOST_FUNCTIONS,
  HOST_PROVIDERS,
  UNRECORDED_PROVIDERS,
  type HostFunction,
} from "./host-functions.ts";
import { NUMERIC_TYPES, numericType } from "./numeric.ts";
import { nominalGenericParts } from "./types.ts";
import type { HirProgram, ValueType } from "./hir.ts";
import { parse, type ParseOptions } from "./parser/index.ts";
import { assembleWat, type WasmArtifact } from "./wasm.ts";
import { RuntimePanicError, runtimePanicName } from "./runtime-panic.ts";
import { calledTraitMethods, reachableProgram, traitMethodKey } from "./emitter/reachability.ts";

/** A checked program and its WAT, before Wasm assembly. */
export interface WatCompilation {
  readonly wat: string;
  readonly hir: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

/** A checked program, its WAT, and the assembled Wasm binary. */
export interface Compilation extends WatCompilation, WasmArtifact {}

interface Analysis {
  readonly hir?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface CompileOptions extends CheckOptions {
  /**
   * Leaves the test cases and test-only functions of a `tests:` block
   * unchecked, as `hd check` does without `--tests` (Testing T42).
   */
  readonly skipTestCode?: boolean;
  /** How to parse `source`: as a test module, or as a linked package's joined modules. */
  readonly parse?: ParseOptions;
}

type SuspensionTraceEvent = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

interface ReplayEventBase {
  readonly siteId: string;
  readonly functionName: string;
  readonly functionCodeId: string;
  readonly functionIndex: number;
  readonly encodedResult: "pending" | "ready";
  readonly providerConfigurationId: string;
}

interface RuntimePollReplayEvent extends ReplayEventBase {
  readonly providerKey: "$runtime";
  readonly operation: "poll";
  readonly encodedArguments: readonly [number];
}

interface HostPollReplayEvent extends ReplayEventBase {
  readonly providerKey: string;
  readonly operation: "provider-poll";
  readonly encodedArguments: readonly EncodedHostValue[];
  readonly encodedValue?: EncodedHostValue;
}

interface EncodedHostInteger {
  readonly kind: "bool" | "char" | "i8" | "i16" | "i32" | "u8" | "u16" | "u32";
  readonly value: number;
}

/** A 64-bit integer, as decimal text, since JSON has no lossless integer. */
interface EncodedHostWide {
  readonly kind: "i64" | "u64";
  readonly value: string;
}

interface EncodedHostFloat {
  readonly bits: string;
  readonly kind: "f32" | "f64";
}

interface EncodedHostString {
  readonly kind: "string";
  readonly utf8: string;
}

/** A `Result[T, E]` boundary result: its tag, and the active side's payload unless `void`. */
interface EncodedHostResult {
  readonly kind: "ok" | "err";
  readonly value?: EncodedHostScalar;
}

type EncodedHostScalar =
  | EncodedHostFloat
  | EncodedHostInteger
  | EncodedHostString
  | EncodedHostWide;

type EncodedHostValue = EncodedHostScalar | EncodedHostResult;

type HostSuspensionValue = number | bigint | string;

/** A host's `Result[T, E]` answer; `value` is absent for a `void` side. */
interface HostResultValue {
  readonly tag: "ok" | "err";
  readonly value?: HostSuspensionValue;
}

type HostSuspensionResult = HostSuspensionValue | HostResultValue;

export type ReplayEvent = HostPollReplayEvent | RuntimePollReplayEvent;

interface ReplaySession {
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
  readonly value?: HostSuspensionResult;
}

/**
 * A provider value that answers its own calls. An embedder passes one for a
 * host requirement when its answers depend on the call it is serving, as
 * the test runner's `TestRunner` and `PropertyRunner` do (src/test-runner.ts).
 */
export interface AnsweringProvider {
  readonly answer: (call: HostSuspensionCall) => HostSuspensionOutcome;
}

function answeringProvider(provider: unknown): AnsweringProvider | undefined {
  return typeof provider === "object" &&
    provider !== null &&
    typeof (provider as Partial<AnsweringProvider>).answer === "function"
    ? (provider as AnsweringProvider)
    : undefined;
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
  /** Host functions that override or add to `HOST_FUNCTIONS`, such as a runner's `snapshot_file_check`. */
  readonly hostFunctions?: Readonly<Record<string, HostFunction>>;
  readonly parse?: ParseOptions;
  /** A compilation of `source` to instantiate again, as for a fresh test instance. */
  readonly compilation?: Compilation;
}

interface Instantiation {
  readonly compilation: Compilation;
  readonly instance: WebAssembly.Instance;
  readonly replay: ReplaySession;
}

/** The `[T, E]` of a `Result[T, E]` boundary result. */
function resultSides(type: ValueType): readonly [ValueType, ValueType] | undefined {
  const parts = nominalGenericParts(type);
  return parts?.name === "Result" && parts.arguments.length === 2
    ? [parts.arguments[0]!, parts.arguments[1]!]
    : undefined;
}

const SCALAR_BOUNDARY = new Set<ValueType>(["bool", "char", "string", ...NUMERIC_TYPES.keys()]);

/** Whether a JavaScript string contains only complete Unicode scalar values. */
function isWellFormedText(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (index + 1 >= value.length || next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

/**
 * Decode a value that Wasm passed to the host. Narrow integers arrive as an
 * i32 bit pattern, so unsigned arguments need normalization before the host
 * sees them. This is deliberately separate from checking values returned by
 * the host: results must never be rounded, truncated, or wrapped to fit.
 */
function hostArgumentValue(type: ValueType, value: HostSuspensionValue): HostSuspensionValue {
  if (type === "string") {
    if (typeof value !== "string") throw new Error("host string boundary value must be a string");
    return value;
  }
  const numeric = numericType(type);
  if (numeric?.wasm === "i64") {
    if (typeof value !== "bigint") throw new Error(`host ${type} boundary value must be a BigInt`);
    return numeric.family === "unsigned" ? BigInt.asUintN(64, value) : BigInt.asIntN(64, value);
  }
  if (numeric?.family === "float") {
    if (typeof value !== "number") throw new Error(`host ${type} boundary value must be a number`);
    return value;
  }
  if (typeof value !== "number") throw new Error(`host ${type} boundary value must be a number`);
  if (type === "bool") return value === 0 ? 0 : 1;
  if (numeric?.family === "unsigned") return value >>> 0;
  return value | 0;
}

/** Validate one scalar supplied by the host without changing it. */
function checkedHostValue(type: ValueType, value: HostSuspensionValue): HostSuspensionValue {
  if (type === "string") {
    if (typeof value !== "string") throw new Error("expected a string");
    if (!isWellFormedText(value)) throw new Error("expected valid Unicode text");
    return value;
  }
  if (type === "bool") {
    if (typeof value !== "number" || (value !== 0 && value !== 1))
      throw new Error("expected bool as 0 or 1");
    return value;
  }
  if (type === "char") {
    if (
      typeof value !== "number" ||
      !Number.isInteger(value) ||
      value < 0 ||
      value > 0x10ffff ||
      (value >= 0xd800 && value <= 0xdfff)
    )
      throw new Error("expected a Unicode scalar value");
    return value;
  }
  const numeric = numericType(type);
  if (numeric?.family === "float") {
    if (typeof value !== "number") throw new Error(`expected a ${type} number`);
    if (type === "f32" && !Object.is(Math.fround(value), value))
      throw new Error(`expected a value already representable as f32, received ${value}`);
    // All values of the declared width are valid, including NaN, infinities,
    // and -0.0. In particular, validation never uses finiteness as a proxy.
    return value;
  }
  if (numeric?.wasm === "i64") {
    if (typeof value !== "bigint") throw new Error(`expected ${type} as a BigInt`);
    if (value < numeric.minimum! || value > numeric.maximum!)
      throw new Error(
        `expected ${type} in ${numeric.minimum}..${numeric.maximum}, received ${value}`,
      );
    return value;
  }
  if (numeric) {
    if (typeof value !== "number" || !Number.isInteger(value))
      throw new Error(`expected an integer ${type}`);
    const integer = BigInt(value);
    if (integer < numeric.minimum! || integer > numeric.maximum!)
      throw new Error(
        `expected ${type} in ${numeric.minimum}..${numeric.maximum}, received ${value}`,
      );
    return value;
  }
  throw new Error(`the host cannot build a '${type}' boundary value`);
}

function checkedHostResult(type: ValueType, value: HostSuspensionResult): HostSuspensionResult {
  const sides = resultSides(type);
  if (!sides) {
    if (typeof value === "object") throw new Error(`host ${type} boundary value must be a scalar`);
    return checkedHostValue(type, value);
  }
  if (value === null || typeof value !== "object" || (value.tag !== "ok" && value.tag !== "err"))
    throw new Error(`host ${type} boundary value must be { tag: "ok" | "err" }`);
  const side = value.tag === "ok" ? sides[0] : sides[1];
  if (side === "void") {
    if (value.value !== undefined)
      throw new Error(`host ${type} '${value.tag}' has a void payload`);
    return { tag: value.tag };
  }
  // An error type the bridge cannot build, such as `ConsoleError`, has no payload.
  if (!SCALAR_BOUNDARY.has(side)) throw new Error(`the host cannot build a '${side}' for ${type}`);
  if (value.value === undefined) throw new Error(`host ${type} result has no '${side}' payload`);
  return { tag: value.tag, value: checkedHostValue(side, value.value) };
}

function checkedLiveHostResult(
  providerMethod: string,
  type: ValueType,
  value: HostSuspensionResult | undefined,
): HostSuspensionResult | undefined {
  try {
    if (type === "void") {
      if (value !== undefined) throw new Error("expected no value for void");
      return undefined;
    }
    if (value === undefined) throw new Error("returned no boundary result");
    return checkedHostResult(type, value);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new RuntimePanicError(
      "host-contract",
      `host provider ${providerMethod} broke its contract: ${reason}`,
    );
  }
}

/** The UTF-8 text a ready result crosses as, if its active side is a `string`. */
function hostResultText(
  type: ValueType,
  value: HostSuspensionResult | undefined,
): string | undefined {
  if (type === "string") return value as string;
  const sides = resultSides(type);
  if (!sides || typeof value !== "object") return undefined;
  return (value.tag === "ok" ? sides[0] : sides[1]) === "string"
    ? (value.value as string)
    : undefined;
}

function encodeHostResult(type: ValueType, value: HostSuspensionResult): EncodedHostValue {
  const sides = resultSides(type);
  if (!sides) return encodeHostValue(type, value as HostSuspensionValue);
  const result = checkedHostResult(type, value) as HostResultValue;
  const side = result.tag === "ok" ? sides[0] : sides[1];
  return result.value === undefined
    ? { kind: result.tag }
    : { kind: result.tag, value: encodeHostValue(side, result.value) as EncodedHostScalar };
}

function decodeHostResult(type: ValueType, encoded: EncodedHostValue): HostSuspensionResult {
  const sides = resultSides(type);
  if (!sides) return decodeHostValue(type, encoded);
  if (encoded.kind !== "ok" && encoded.kind !== "err")
    throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
  const side = encoded.kind === "ok" ? sides[0] : sides[1];
  const result: HostResultValue =
    encoded.value === undefined
      ? { tag: encoded.kind }
      : { tag: encoded.kind, value: decodeHostValue(side, encoded.value) };
  return checkedHostResult(type, result);
}

function hexBytes(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function bytesFromHex(value: string): Uint8Array {
  if (typeof value !== "string" || !/^(?:[0-9a-f]{2})*$/.test(value))
    throw new Error(`replay UTF-8 bytes '${value}' are invalid`);
  return Uint8Array.from(
    Array.from({ length: value.length / 2 }, (_, index) =>
      Number.parseInt(value.slice(index * 2, index * 2 + 2), 16),
    ),
  );
}

function encodeHostValue(type: ValueType, value: HostSuspensionValue): EncodedHostValue {
  const canonical = hostArgumentValue(type, value);
  if (type === "string")
    return { kind: "string", utf8: hexBytes(new TextEncoder().encode(canonical as string)) };
  const numeric = numericType(type);
  if (numeric?.wasm === "i64")
    return { kind: type as EncodedHostWide["kind"], value: String(canonical) };
  if (numeric?.family !== "float")
    return { kind: type as EncodedHostInteger["kind"], value: Number(canonical) };
  if (Number.isNaN(canonical))
    return {
      bits: type === "f32" ? "7fc00000" : "7ff8000000000000",
      kind: type as EncodedHostFloat["kind"],
    };
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  if (type === "f32") {
    view.setFloat32(0, Number(canonical), false);
    return { bits: view.getUint32(0, false).toString(16).padStart(8, "0"), kind: "f32" };
  }
  view.setFloat64(0, Number(canonical), false);
  return {
    bits: view.getBigUint64(0, false).toString(16).padStart(16, "0"),
    kind: "f64",
  };
}

function decodeHostValue(type: ValueType, value: EncodedHostValue): HostSuspensionValue {
  if (value.kind !== type || value.kind === "ok" || value.kind === "err")
    throw new Error(`replay boundary type '${value.kind}' does not match '${type}'`);
  const encoded = value as EncodedHostScalar;
  if (encoded.kind === "string") {
    if (typeof encoded.utf8 !== "string")
      throw new Error("replay string boundary value is not lowercase hexadecimal");
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(
      bytesFromHex(encoded.utf8),
    );
  }
  if (encoded.kind === "i64" || encoded.kind === "u64") {
    if (typeof encoded.value !== "string")
      throw new Error(`replay ${encoded.kind} value is not decimal text`);
    const valid =
      encoded.kind === "i64" ? /^-?\d+$/.test(encoded.value) : /^\d+$/.test(encoded.value);
    if (!valid) throw new Error(`replay ${encoded.kind} value '${encoded.value}' is invalid`);
    return checkedHostValue(type, BigInt(encoded.value));
  }
  if (!("bits" in encoded)) return checkedHostValue(type, encoded.value);
  const digits = encoded.kind === "f32" ? 8 : 16;
  if (typeof encoded.bits !== "string" || !new RegExp(`^[0-9a-f]{${digits}}$`).test(encoded.bits))
    throw new Error(`replay ${encoded.kind} bits '${encoded.bits}' are invalid`);
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  if (encoded.kind === "f32") {
    view.setUint32(0, Number.parseInt(encoded.bits, 16), false);
    return view.getFloat32(0, false);
  }
  view.setBigUint64(0, BigInt(`0x${encoded.bits}`), false);
  return view.getFloat64(0, false);
}

function sameEncodedHostValue(left: EncodedHostValue, right: EncodedHostValue): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "ok" || left.kind === "err" || right.kind === "ok" || right.kind === "err")
    return JSON.stringify(left) === JSON.stringify(right);
  if ("bits" in left) return "bits" in right && left.bits === right.bits;
  if (left.kind === "string") return right.kind === "string" && left.utf8 === right.utf8;
  return "value" in right && left.value === right.value;
}

function validateReplayHostOutcome(
  providerMethod: string,
  resultType: ValueType,
  suspending: boolean,
  encodedResult: unknown,
  encodedValue: EncodedHostValue | undefined,
): asserts encodedResult is HostPollReplayEvent["encodedResult"] {
  if (encodedResult !== "pending" && encodedResult !== "ready")
    throw new Error(
      `replay provider ${providerMethod} has invalid result state '${String(encodedResult)}'`,
    );
  if (encodedResult === "pending" && !suspending)
    throw new Error(`replay provider ${providerMethod} is a plain call, not pending`);
  if (encodedResult === "pending" && encodedValue !== undefined)
    throw new Error(`replay provider ${providerMethod} has a result while pending`);
  if (encodedResult === "ready" && resultType === "void" && encodedValue !== undefined)
    throw new Error(`replay provider ${providerMethod} has a boundary result for void`);
  if (encodedResult === "ready" && resultType !== "void" && encodedValue === undefined)
    throw new Error(`replay provider ${providerMethod} has no boundary result`);
}

export function analyze(source: string, options: CompileOptions = {}): Analysis {
  try {
    const parsed = parse(source, options.parse);
    if (!parsed.program) return { diagnostics: parsed.diagnostics };
    const program = options.skipTestCode
      ? {
          ...parsed.program,
          testCode: false,
          tests: [],
          functions: parsed.program.functions.filter((declaration) => !declaration.testOnly),
        }
      : parsed.program;
    const checked = check(program, options);
    return { hir: checked.program, diagnostics: checked.diagnostics };
  } catch (error) {
    if (error instanceof DiagnosticError) return { diagnostics: error.diagnostics };
    throw error;
  }
}

/**
 * Parses, checks, and lowers `source`, and emits its WAT: everything short of
 * Wasm assembly. It never loads Binaryen, so `hd check`, `hd build --wat`,
 * and the debug commands stay fast. Throws a {@link DiagnosticError} when the
 * program does not check.
 */
export function compileToWat(source: string, options: CompileOptions = {}): WatCompilation {
  const analysis = analyze(source, options);
  if (!analysis.hir) throw new DiagnosticError(analysis.diagnostics);
  return { wat: emitWat(analysis.hir), hir: analysis.hir, diagnostics: analysis.diagnostics };
}

/**
 * `compileToWat`, then assembles the WAT into a validated Wasm binary. Pass
 * a {@link WatCompilation} to assemble one already compiled. Binaryen loads
 * on the first call.
 */
export async function compileToWasm(
  input: string | WatCompilation,
  options: CompileOptions = {},
): Promise<Compilation> {
  const compilation = typeof input === "string" ? compileToWat(input, options) : input;
  const { bytes } = await assembleWat(compilation.wat);
  return { ...compilation, bytes };
}

/**
 * The imports that read a ready host capability result (emitter/host-providers.ts):
 * a scalar, a string's UTF-8 bytes, or a `Result[T, E]`'s tag and payload.
 */
function hostResultImports(
  prefix: string,
  name: string,
  type: ValueType,
): Record<string, HostImport> {
  const imports: Record<string, HostImport> = {};
  const sides = resultSides(type);
  const ready = (value: unknown): HostSuspensionResult => {
    const state = value as HostCallState;
    if (!state.outcome || state.outcome.pending || state.outcome.value === undefined)
      throw new Error(`host provider ${name} has no ready result`);
    return state.outcome.value;
  };
  const bytes = (value: unknown): Uint8Array => {
    const state = value as HostCallState;
    if (!state.resultBytes) throw new Error(`host provider ${name} has no ready result`);
    return state.resultBytes;
  };
  if (sides) {
    imports[`${prefix}_result_tag`] = (value) =>
      (ready(value) as HostResultValue).tag === "ok" ? 0 : 1;
    imports[`${prefix}_result_ok`] = (value) => (ready(value) as HostResultValue).value;
    imports[`${prefix}_result_err`] = (value) => (ready(value) as HostResultValue).value;
  }
  if (type === "string" || sides?.includes("string")) {
    imports[`${prefix}_result_length`] = (value) => bytes(value).length;
    imports[`${prefix}_result_byte`] = (value, index) => bytes(value)[Number(index)];
  }
  if (!sides && type !== "void" && type !== "string")
    imports[`${prefix}_result`] = (value) => ready(value);
  return imports;
}

export async function instantiate(
  source: string,
  options: InstantiateOptions = {},
): Promise<Instantiation> {
  const compilation = options.compilation ?? (await compileToWasm(source, options));
  const functionIdentities: FunctionIdentity[] = [
    ...compilation.hir.functions,
    ...compilation.hir.closures,
  ].map((declaration) => {
    const span = physicalSpan(declaration.span);
    const text = sourceDocument(declaration.span)?.text ?? source;
    return {
      codeId: createHash("sha256")
        .update(declaration.name)
        .update("\0")
        .update(text.slice(span.start.offset, span.end.offset))
        .digest("hex")
        .slice(0, 16),
      end: span.end.offset,
      index: declaration.suspensionIndex ?? declaration.index,
      name: declaration.name,
      start: span.start.offset,
    };
  });
  const functionIdentity = (index: number): FunctionIdentity | undefined =>
    functionIdentities.find((identity) => identity.index === index);
  const configurationId = options.providerConfigurationId ?? "default";
  const textEncoder = new TextEncoder();
  // Strings are UTF-8 at every host boundary, so a leading U+FEFF is text, not
  // a byte order mark to drop.
  const textDecoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
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
  const hostCapabilities = new Set(compilation.hir.hostCapabilities);
  const calledHostMethods = calledTraitMethods(reachableProgram(compilation.hir));
  const hostCall = (
    provider: unknown,
    providerKey: string,
    methodName: string,
    functionIndex: number,
    siteOffset: number,
    arguments_: HostSuspensionValue[],
  ): MutableHostSuspensionCall => {
    const identity = functionIdentity(functionIndex);
    if (!identity)
      throw new Error(`host provider call has unknown function index ${functionIndex}`);
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
    // An unrecorded provider's calls are neither recorded nor replayed
    // (host-functions.ts). A profile may hold even a built-in call pending;
    // once released, the built-in supplies the ready answer.
    const recorded = !UNRECORDED_PROVIDERS.has(trait.name);
    for (const method of trait.methods) {
      if (!calledHostMethods.has(traitMethodKey(trait.index, method.index))) continue;
      const prefix = `host_${trait.index}_${method.index}`;
      const builtIn = HOST_PROVIDERS[`${trait.name}.${method.name}`];
      hostImports[`${prefix}_begin`] = (provider, functionIndex, siteOffset, ...arguments_) => ({
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
          functionIndex as number,
          siteOffset as number,
          method.parameters.map((parameter, index) =>
            parameter === "string"
              ? ""
              : hostArgumentValue(
                  parameter,
                  numericType(parameter)?.wasm === "i64"
                    ? (arguments_[index] as bigint)
                    : Number(arguments_[index]),
                ),
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
        const expected = recorded ? options.replay?.[replayIndex] : undefined;
        if (options.replay && recorded) {
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
          validateReplayHostOutcome(
            `${trait.name}.${method.name}`,
            method.result,
            method.suspending,
            expected.encodedResult,
            expected.encodedValue,
          );
          state.outcome = {
            pending: expected.encodedResult === "pending",
            ...(expected.encodedValue
              ? { value: decodeHostResult(method.result, expected.encodedValue) }
              : {}),
          };
          const text = state.outcome.pending
            ? undefined
            : hostResultText(method.result, state.outcome.value);
          if (text !== undefined) state.resultBytes = textEncoder.encode(text);
          replayIndex += 1;
          return state.outcome.pending ? 0 : 1;
        }
        const answering = answeringProvider(call.provider);
        const heldPending = options.hostSuspensionPending?.(call) ?? false;
        state.outcome = heldPending
          ? { pending: true }
          : builtIn
            ? builtIn(call, { console: options.console })
            : answering
              ? answering.answer(call)
              : (options.hostSuspensionInvoke?.(call) ?? { pending: false });
        // A plain method has no suspension to leave pending
        // (emitter/host-providers.ts).
        if (state.outcome.pending && !method.suspending)
          throw new Error(
            `host provider ${trait.name}.${method.name} is a plain call, not pending`,
          );
        if (state.outcome.pending) state.outcome = { pending: true };
        else {
          const value = checkedLiveHostResult(
            `${trait.name}.${method.name}`,
            method.result,
            state.outcome.value,
          );
          state.outcome = {
            pending: false,
            ...(value === undefined ? {} : { value }),
          };
        }
        const text = state.outcome.pending
          ? undefined
          : hostResultText(method.result, state.outcome.value);
        if (text !== undefined) state.resultBytes = textEncoder.encode(text);
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
            : { encodedValue: encodeHostResult(method.result, state.outcome.value) }),
          providerConfigurationId: configurationId,
        };
        if (recorded) options.record?.(event);
        return state.outcome.pending ? 0 : 1;
      };
      hostImports[`${prefix}_cancel`] = (value) => {
        if (!builtIn) options.hostSuspensionCancel?.((value as HostCallState).call);
      };
      Object.assign(
        hostImports,
        hostResultImports(prefix, `${trait.name}.${method.name}`, method.result),
      );
    }
  }
  // The generic host-function boundary (host-functions.ts): a `string`
  // crosses as a handle whose UTF-8 bytes the Wasm side copies one by one.
  interface HostString {
    readonly bytes: Uint8Array;
  }
  hostImports.host_string_new = (length) => ({ bytes: new Uint8Array(Number(length)) });
  hostImports.host_string_set = (handle, index, byte) => {
    (handle as HostString).bytes[Number(index)] = Number(byte);
  };
  hostImports.host_string_length = (handle) => (handle as HostString).bytes.length;
  hostImports.host_string_get = (handle, index) => (handle as HostString).bytes[Number(index)];
  for (const declaration of compilation.hir.functions) {
    const name = declaration.intrinsic;
    if (!name || isRuntimePrimitive(name)) continue;
    hostImports[`host:${name}`] = (...arguments_) => {
      const implementation = options.hostFunctions?.[name] ?? HOST_FUNCTIONS[name];
      if (!implementation) throw new Error(`the host has no function '${name}'`);
      const result = implementation(
        ...declaration.parameters.map((parameter, index) =>
          parameter.type === "string"
            ? textDecoder.decode((arguments_[index] as HostString).bytes)
            : (arguments_[index] as number | bigint),
        ),
      );
      if (declaration.result === "string")
        return { bytes: textEncoder.encode(String(result)) } satisfies HostString;
      if (declaration.result === "bool") return result ? 1 : 0;
      return result;
    };
  }
  const { instance } = await WebAssembly.instantiate(compilation.bytes, {
    hd: {
      ...hostImports,
      trace: options.trace ?? (() => undefined),
      pending,
      pow_f64: Math.pow,
      // JavaScript `%` on numbers is the truncated remainder of C `fmod`.
      rem_f64: (left: number, right: number) => left % right,
      panic: (code: number) => {
        throw new RuntimePanicError(runtimePanicName(code));
      },
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
