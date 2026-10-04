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
import { payloadlessSingletonEnum } from "./host-boundary.ts";
import { NUMERIC_TYPES, numericType } from "./numeric.ts";
import {
  nominalGenericParts,
  optionalInner,
  substituteTypeParameters,
  tupleParts,
} from "./types.ts";
import type { HirEnum, HirProgram, HirTraitMethod, ValueType } from "./hir.ts";
import { parse, type ParseOptions } from "./parser/index.ts";
import { assembleWat, type WasmArtifact } from "./wasm.ts";
import { RuntimePanicError, runtimePanicName } from "./runtime-panic.ts";
import { emissionReachability, traitMethodKey } from "./emitter/reachability.ts";

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
  readonly value?: EncodedHostValue;
}

interface EncodedHostOptional {
  readonly kind: "none" | "some";
  readonly value?: EncodedHostValue;
}

interface EncodedHostSequence {
  readonly kind: "list" | "tuple";
  readonly values: readonly EncodedHostValue[];
}

interface EncodedHostData {
  readonly kind: "data";
  readonly name: string;
  readonly fields: readonly { readonly name: string; readonly value: EncodedHostValue }[];
}

type EncodedHostScalar =
  | EncodedHostFloat
  | EncodedHostInteger
  | EncodedHostString
  | EncodedHostWide;

type EncodedHostValue =
  | EncodedHostData
  | EncodedHostOptional
  | EncodedHostResult
  | EncodedHostScalar
  | EncodedHostSequence;

type HostSuspensionValue = number | bigint | string;

export type HostBoundaryValue =
  | HostSuspensionValue
  | HostResultValue
  | { readonly tag: "none" }
  | { readonly tag: "some"; readonly value: HostBoundaryValue }
  | readonly HostBoundaryValue[]
  | { readonly [field: string]: HostBoundaryValue };

/** A host's `Result[T, E]` answer; `value` is absent for a `void` side. */
interface HostResultValue {
  readonly tag: "ok" | "err";
  readonly value?: HostSuspensionValue;
}

type HostSuspensionResult = HostBoundaryValue;

type HostBoundaryNode =
  | { readonly kind: "scalar"; readonly type: ValueType; readonly value: HostSuspensionValue }
  | {
      readonly kind: "variant";
      readonly tag: number;
      readonly children: readonly HostBoundaryNode[];
    }
  | { readonly kind: "sequence"; readonly children: readonly HostBoundaryNode[] }
  | { readonly kind: "record"; readonly children: readonly HostBoundaryNode[] };

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
  readonly resultType: ValueType;
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
  resultNode?: HostBoundaryNode;
  resultBytes?: Uint8Array;
}

interface HostString {
  readonly bytes: Uint8Array;
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
  readonly consoleError?: (text: string, provider: unknown) => void;
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

function hostObject(value: HostBoundaryValue): Record<string, HostBoundaryValue> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error("expected an object");
  return value as Record<string, HostBoundaryValue>;
}

function withBoundaryObject<T>(
  value: HostBoundaryValue,
  ancestors: Set<object>,
  check: () => T,
): T {
  if (value === null || typeof value !== "object") return check();
  if (ancestors.has(value)) throw new Error("boundary values must not contain a cycle");
  ancestors.add(value);
  try {
    return check();
  } finally {
    ancestors.delete(value);
  }
}

function checkedHostBoundaryNode(
  program: HirProgram,
  type: ValueType,
  value: HostBoundaryValue,
  ancestors: Set<object> = new Set(),
): HostBoundaryNode {
  if (SCALAR_BOUNDARY.has(type)) {
    if (typeof value === "object") throw new Error(`expected a ${type} scalar`);
    return { kind: "scalar", type, value: checkedHostValue(type, value) };
  }
  const optional = optionalInner(type);
  if (optional !== undefined)
    return withBoundaryObject(value, ancestors, () => {
      const tagged = hostObject(value);
      if (tagged.tag === "none") {
        if (Object.keys(tagged).length !== 1) throw new Error(`expected '${type}' .None`);
        return { kind: "variant", tag: 0, children: [] };
      }
      if (tagged.tag !== "some" || !("value" in tagged) || Object.keys(tagged).length !== 2)
        throw new Error(`expected '${type}' as { tag: "none" | "some", value? }`);
      return {
        kind: "variant",
        tag: 1,
        children: [checkedHostBoundaryNode(program, optional, tagged.value!, ancestors)],
      };
    });
  const tuple = tupleParts(type);
  if (tuple)
    return withBoundaryObject(value, ancestors, () => {
      if (!Array.isArray(value) || value.length !== tuple.length)
        throw new Error(`expected '${type}' as an array of ${tuple.length} values`);
      return {
        kind: "sequence",
        children: tuple.map((element, index) =>
          checkedHostBoundaryNode(program, element, value[index]!, ancestors),
        ),
      };
    });
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1)
    return withBoundaryObject(value, ancestors, () => {
      if (!Array.isArray(value)) throw new Error(`expected '${type}' as an array`);
      return {
        kind: "sequence",
        children: value.map((element) =>
          checkedHostBoundaryNode(program, nominal.arguments[0]!, element, ancestors),
        ),
      };
    });
  const dataName = nominal?.name ?? type;
  const data = program.data.find((declaration) => declaration.name === dataName);
  if (data)
    return withBoundaryObject(value, ancestors, () => {
      const object = hostObject(value);
      const expected = new Set(data.fields.map((field) => field.name));
      const extra = Object.keys(object).find((name) => !expected.has(name));
      if (extra) throw new Error(`host '${dataName}' has an unknown field '${extra}'`);
      const arguments_ = nominal?.arguments ?? [];
      const substitutions = new Map(
        data.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
      );
      return {
        kind: "record",
        children: data.fields.map((field) => {
          if (!field.public && !field.embedded)
            throw new Error(`host '${dataName}' cannot set private field '${field.name}'`);
          if (!(field.name in object))
            throw new Error(`host '${dataName}' is missing field '${field.name}'`);
          return checkedHostBoundaryNode(
            program,
            substituteTypeParameters(field.type, substitutions),
            object[field.name]!,
            ancestors,
          );
        }),
      };
    });
  throw new Error(`the host cannot build a '${type}' boundary value`);
}

function structuralHostResult(program: HirProgram, type: ValueType): boolean {
  if (optionalInner(type) !== undefined || tupleParts(type) !== undefined) return true;
  const nominal = nominalGenericParts(type);
  return (
    (nominal?.name === "List" && nominal.arguments.length === 1) ||
    program.data.some((declaration) => declaration.name === (nominal?.name ?? type))
  );
}

function checkedHostResult(
  type: ValueType,
  value: HostSuspensionResult,
  enums: readonly HirEnum[],
): HostSuspensionResult {
  const sides = resultSides(type);
  if (!sides) {
    if (typeof value === "object") throw new Error(`host ${type} boundary value must be a scalar`);
    return checkedHostValue(type, value);
  }
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error(`host ${type} boundary value must be { tag: "ok" | "err" }`);
  const result = value as HostResultValue;
  if (result.tag !== "ok" && result.tag !== "err")
    throw new Error(`host ${type} boundary value must be { tag: "ok" | "err" }`);
  const side = result.tag === "ok" ? sides[0] : sides[1];
  if (side === "void") {
    if (result.value !== undefined)
      throw new Error(`host ${type} '${result.tag}' has a void payload`);
    return { tag: result.tag };
  }
  if (payloadlessSingletonEnum(enums, side)) {
    if (result.value !== undefined)
      throw new Error(`host ${type} '${result.tag}' has an implicit enum payload`);
    return { tag: result.tag };
  }
  if (!SCALAR_BOUNDARY.has(side)) throw new Error(`the host cannot build a '${side}' for ${type}`);
  if (result.value === undefined) throw new Error(`host ${type} result has no '${side}' payload`);
  return { tag: result.tag, value: checkedHostValue(side, result.value) };
}

function checkedLiveHostResult(
  providerMethod: string,
  type: ValueType,
  value: HostSuspensionResult | undefined,
  enums: readonly HirEnum[],
): HostSuspensionResult | undefined {
  try {
    if (type === "void") {
      if (value !== undefined) throw new Error("expected no value for void");
      return undefined;
    }
    if (value === undefined) throw new Error("returned no boundary result");
    return checkedHostResult(type, value, enums);
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
  const result = value as HostResultValue;
  return (result.tag === "ok" ? sides[0] : sides[1]) === "string"
    ? (result.value as string)
    : undefined;
}

function encodeHostResult(
  type: ValueType,
  value: HostSuspensionResult,
  enums: readonly HirEnum[],
): EncodedHostValue {
  const sides = resultSides(type);
  if (!sides) return encodeHostValue(type, value as HostSuspensionValue);
  const result = checkedHostResult(type, value, enums) as HostResultValue;
  const side = result.tag === "ok" ? sides[0] : sides[1];
  return result.value === undefined
    ? { kind: result.tag }
    : { kind: result.tag, value: encodeHostValue(side, result.value) as EncodedHostScalar };
}

function decodeHostResult(
  type: ValueType,
  encoded: EncodedHostValue,
  enums: readonly HirEnum[],
): HostSuspensionResult {
  const sides = resultSides(type);
  if (!sides) return decodeHostValue(type, encoded);
  if (encoded.kind !== "ok" && encoded.kind !== "err")
    throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
  const side = encoded.kind === "ok" ? sides[0] : sides[1];
  const result: HostResultValue =
    encoded.value === undefined
      ? { tag: encoded.kind }
      : { tag: encoded.kind, value: decodeHostValue(side, encoded.value) };
  return checkedHostResult(type, result, enums);
}

function encodeHostBoundaryNode(
  program: HirProgram,
  type: ValueType,
  node: HostBoundaryNode,
): EncodedHostValue {
  if (SCALAR_BOUNDARY.has(type)) {
    if (node.kind !== "scalar") throw new Error(`host boundary node does not match '${type}'`);
    return encodeHostValue(type, node.value);
  }
  const optional = optionalInner(type);
  if (optional !== undefined) {
    if (node.kind !== "variant" || (node.tag !== 0 && node.tag !== 1))
      throw new Error(`host boundary node does not match '${type}'`);
    return node.tag === 0
      ? { kind: "none" }
      : {
          kind: "some",
          value: encodeHostBoundaryNode(program, optional, node.children[0]!),
        };
  }
  const tuple = tupleParts(type);
  if (tuple) {
    if (node.kind !== "sequence" || node.children.length !== tuple.length)
      throw new Error(`host boundary node does not match '${type}'`);
    return {
      kind: "tuple",
      values: tuple.map((element, index) =>
        encodeHostBoundaryNode(program, element, node.children[index]!),
      ),
    };
  }
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1) {
    if (node.kind !== "sequence") throw new Error(`host boundary node does not match '${type}'`);
    return {
      kind: "list",
      values: node.children.map((child) =>
        encodeHostBoundaryNode(program, nominal.arguments[0]!, child),
      ),
    };
  }
  const dataName = nominal?.name ?? type;
  const data = program.data.find((declaration) => declaration.name === dataName);
  if (!data || node.kind !== "record" || node.children.length !== data.fields.length)
    throw new Error(`host boundary node does not match '${type}'`);
  const arguments_ = nominal?.arguments ?? [];
  const substitutions = new Map(
    data.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
  );
  return {
    kind: "data",
    name: data.standardName ?? data.name,
    fields: data.fields.map((field, index) => ({
      name: field.name,
      value: encodeHostBoundaryNode(
        program,
        substituteTypeParameters(field.type, substitutions),
        node.children[index]!,
      ),
    })),
  };
}

function decodeHostBoundaryNode(
  program: HirProgram,
  type: ValueType,
  encoded: EncodedHostValue,
): HostBoundaryNode {
  if (SCALAR_BOUNDARY.has(type))
    return { kind: "scalar", type, value: decodeHostValue(type, encoded) };
  const optional = optionalInner(type);
  if (optional !== undefined) {
    if (encoded.kind === "none") {
      if (encoded.value !== undefined)
        throw new Error(`replay boundary type 'none' has a payload for '${type}'`);
      return { kind: "variant", tag: 0, children: [] };
    }
    if (encoded.kind !== "some" || encoded.value === undefined)
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "variant",
      tag: 1,
      children: [decodeHostBoundaryNode(program, optional, encoded.value)],
    };
  }
  const tuple = tupleParts(type);
  if (tuple) {
    if (
      encoded.kind !== "tuple" ||
      !Array.isArray(encoded.values) ||
      encoded.values.length !== tuple.length
    )
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "sequence",
      children: tuple.map((element, index) =>
        decodeHostBoundaryNode(program, element, encoded.values[index]!),
      ),
    };
  }
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1) {
    if (encoded.kind !== "list" || !Array.isArray(encoded.values))
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "sequence",
      children: encoded.values.map((value) =>
        decodeHostBoundaryNode(program, nominal.arguments[0]!, value),
      ),
    };
  }
  const dataName = nominal?.name ?? type;
  const data = program.data.find((declaration) => declaration.name === dataName);
  if (
    !data ||
    encoded.kind !== "data" ||
    encoded.name !== (data.standardName ?? data.name) ||
    !Array.isArray(encoded.fields)
  )
    throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
  if (
    encoded.fields.length !== data.fields.length ||
    encoded.fields.some((field, index) => field.name !== data.fields[index]!.name)
  )
    throw new Error(`replay data fields do not match '${type}'`);
  const arguments_ = nominal?.arguments ?? [];
  const substitutions = new Map(
    data.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
  );
  return {
    kind: "record",
    children: data.fields.map((field, index) =>
      decodeHostBoundaryNode(
        program,
        substituteTypeParameters(field.type, substitutions),
        encoded.fields[index]!.value,
      ),
    ),
  };
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
  if (
    left.kind === "ok" ||
    left.kind === "err" ||
    left.kind === "none" ||
    left.kind === "some" ||
    left.kind === "list" ||
    left.kind === "tuple" ||
    left.kind === "data" ||
    right.kind === "ok" ||
    right.kind === "err" ||
    right.kind === "none" ||
    right.kind === "some" ||
    right.kind === "list" ||
    right.kind === "tuple" ||
    right.kind === "data"
  )
    return JSON.stringify(left) === JSON.stringify(right);
  if ("bits" in left) return "bits" in right && left.bits === right.bits;
  if (left.kind === "string") return right.kind === "string" && left.utf8 === right.utf8;
  return "value" in left && "value" in right && left.value === right.value;
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

function hostPollEvent(
  program: HirProgram,
  call: HostSuspensionCall,
  method: HirTraitMethod,
  outcome: HostSuspensionOutcome,
  configurationId: string,
  enums: readonly HirEnum[],
): HostPollReplayEvent {
  return {
    siteId: call.siteId,
    functionName: call.functionName,
    functionCodeId: call.functionCodeId,
    functionIndex: call.functionIndex,
    providerKey: call.providerKey,
    operation: "provider-poll",
    encodedArguments: call.arguments.map((argument, index) =>
      encodeHostValue(method.parameters[index]!, argument),
    ),
    encodedResult: outcome.pending ? "pending" : "ready",
    ...(outcome.value === undefined
      ? {}
      : {
          encodedValue: structuralHostResult(program, method.result)
            ? encodeHostBoundaryNode(
                program,
                method.result,
                checkedHostBoundaryNode(program, method.result, outcome.value),
              )
            : encodeHostResult(method.result, outcome.value, enums),
        }),
    providerConfigurationId: configurationId,
  };
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
  program: HirProgram,
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
  if (structuralHostResult(program, type)) {
    imports[`${prefix}_result_node`] = (value) => {
      const node = (value as HostCallState).resultNode;
      if (!node) throw new Error(`host provider ${name} has no ready structural result`);
      return node;
    };
    return imports;
  }
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

function structuralBoundaryImports(textEncoder: TextEncoder): Record<string, HostImport> {
  const boundaryNode = (value: unknown): HostBoundaryNode => value as HostBoundaryNode;
  const boundaryScalar = (value: unknown): HostSuspensionValue => {
    const node = boundaryNode(value);
    if (node.kind !== "scalar") throw new Error("host boundary node is not a scalar");
    return node.value;
  };
  return {
    host_boundary_tag(value) {
      const node = boundaryNode(value);
      if (node.kind !== "variant") throw new Error("host boundary node is not a variant");
      return node.tag;
    },
    host_boundary_length(value) {
      const node = boundaryNode(value);
      if (node.kind !== "sequence" && node.kind !== "record")
        throw new Error("host boundary node has no children");
      return node.children.length;
    },
    host_boundary_child(value, index) {
      const node = boundaryNode(value);
      if (node.kind !== "sequence" && node.kind !== "record" && node.kind !== "variant")
        throw new Error("host boundary node has no children");
      const child = node.children[Number(index)];
      if (!child) throw new Error("host boundary child index is out of bounds");
      return child;
    },
    host_boundary_i32: boundaryScalar,
    host_boundary_i64: boundaryScalar,
    host_boundary_f32: boundaryScalar,
    host_boundary_f64: boundaryScalar,
    host_boundary_string_length: (value) =>
      textEncoder.encode(String(boundaryScalar(value))).length,
    host_boundary_string_byte: (value, index) =>
      textEncoder.encode(String(boundaryScalar(value)))[Number(index)],
  };
}

function makeHostCall(
  functionIdentity: (index: number) => FunctionIdentity | undefined,
  provider: unknown,
  providerKey: string,
  methodName: string,
  resultType: ValueType,
  functionIndex: number,
  siteOffset: number,
  arguments_: HostSuspensionValue[],
): MutableHostSuspensionCall {
  const identity = functionIdentity(functionIndex);
  if (!identity) throw new Error(`host provider call has unknown function index ${functionIndex}`);
  return {
    arguments: arguments_,
    functionCodeId: identity.codeId,
    functionIndex: identity.index,
    functionName: identity.name,
    methodName,
    provider,
    providerKey,
    resultType,
    siteId: `${identity.name}:provider:${providerKey}.${methodName}:${siteOffset - identity.start}`,
  };
}

function runtimeFunctionIdentities(source: string, program: HirProgram): FunctionIdentity[] {
  return [...program.functions, ...program.closures].map((declaration) => {
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
}

export async function instantiate(
  source: string,
  options: InstantiateOptions = {},
): Promise<Instantiation> {
  const compilation = options.compilation ?? (await compileToWasm(source, options));
  const hostEnums = compilation.hir.enums;
  const functionIdentities = runtimeFunctionIdentities(source, compilation.hir);
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
  hostImports.panic_with_message = (code, message) => {
    const bytes = (message as HostString).bytes;
    throw new RuntimePanicError(runtimePanicName(Number(code)), textDecoder.decode(bytes));
  };
  Object.assign(hostImports, structuralBoundaryImports(textEncoder));
  const hostCapabilities = new Set(compilation.hir.hostCapabilities);
  const calledHostMethods = emissionReachability(compilation.hir).traitMethods;
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
        call: makeHostCall(
          functionIdentity,
          provider,
          trait.name,
          method.name,
          method.result,
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
            ...(expected.encodedValue && !structuralHostResult(compilation.hir, method.result)
              ? {
                  value: decodeHostResult(method.result, expected.encodedValue, hostEnums),
                }
              : {}),
          };
          if (
            !state.outcome.pending &&
            expected.encodedValue &&
            structuralHostResult(compilation.hir, method.result)
          )
            state.resultNode = decodeHostBoundaryNode(
              compilation.hir,
              method.result,
              expected.encodedValue,
            );
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
            ? builtIn(call, {
                console: options.console,
                consoleError: options.consoleError,
              })
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
          const value = structuralHostResult(compilation.hir, method.result)
            ? (() => {
                if (state.outcome!.value === undefined)
                  throw new RuntimePanicError(
                    "host-contract",
                    `host provider ${trait.name}.${method.name} broke its contract: returned no boundary result`,
                  );
                try {
                  state.resultNode = checkedHostBoundaryNode(
                    compilation.hir,
                    method.result,
                    state.outcome!.value,
                  );
                  return state.outcome!.value;
                } catch (error) {
                  const reason = error instanceof Error ? error.message : String(error);
                  throw new RuntimePanicError(
                    "host-contract",
                    `host provider ${trait.name}.${method.name} broke its contract: ${reason}`,
                  );
                }
              })()
            : checkedLiveHostResult(
                `${trait.name}.${method.name}`,
                method.result,
                state.outcome.value,
                hostEnums,
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
        const event = hostPollEvent(
          compilation.hir,
          call,
          method,
          state.outcome,
          configurationId,
          hostEnums,
        );
        if (recorded) options.record?.(event);
        return state.outcome.pending ? 0 : 1;
      };
      hostImports[`${prefix}_cancel`] = (value) => {
        if (!builtIn) options.hostSuspensionCancel?.((value as HostCallState).call);
      };
      Object.assign(
        hostImports,
        hostResultImports(compilation.hir, prefix, `${trait.name}.${method.name}`, method.result),
      );
    }
  }
  // The generic host-function boundary (host-functions.ts): a `string`
  // crosses as a handle whose UTF-8 bytes the Wasm side copies one by one.
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
