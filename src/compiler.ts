import type { Diagnostic } from "./diagnostics.ts";
import { DiagnosticError } from "./diagnostics.ts";
import { check, type CheckOptions } from "./checker/index.ts";
import { emitWat } from "./emitter/index.ts";
import {
  hostStringImports,
  HOST_FUNCTIONS,
  HOST_PROVIDERS,
  UNRECORDED_PROVIDERS,
  type HostFunction,
} from "./host-functions.ts";
import {
  boundaryFieldsVisible,
  boundaryShape,
  isBoundaryScalar,
  isStringListArgument,
  payloadlessSingletonEnum,
  programLookups,
  resultSides,
  structuralHostArgument,
  structuralHostResult,
  checkedModule,
  type BoundaryTypes,
} from "./host-boundary.ts";
import {
  argumentBuffers,
  argumentTokenImports,
  decodeStreamedArguments,
  streamedArgumentImports,
  type ArgumentBuffers,
} from "./host-arguments.ts";
import { checkedHostValue, hostArgumentValue } from "./host-values.ts";
import { numericType } from "./numeric.ts";
import { substituteTypeParameters } from "./types.ts";
import type { HirEnum, HirProgram, HirTrait, ValueType } from "./hir.ts";
import { parse, type ParseOptions } from "./parser/index.ts";
import { assembleWat, type WasmArtifact } from "./wasm.ts";
import { RuntimePanicError, runtimePanicName, type PanicSite } from "./runtime-panic.ts";
import { panicLocator, stackExhaustionPanics } from "./panic-locator.ts";
import {
  runtimeInterface,
  type FunctionIdentity,
  type HostMethodInterface,
  type RuntimeInterface,
} from "./runtime-interface.ts";

/** A checked program and its WAT, before Wasm assembly. */
interface WatCompilation {
  readonly wat: string;
  readonly hir: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
  /** The panic sites the WAT annotates (emitter/panic-sites.ts). */
  readonly sites?: readonly PanicSite[];
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
  /**
   * A release build: integer overflow wraps and shift counts are masked. The default is a
   * debug build, which panics (spec/lang/04-type-system.md#r-types.arith.checked).
   */
  readonly release?: boolean;
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
  readonly kind: "bool" | "char" | "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "usize";
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

/** An enum value: its enum, its variant, and that variant's fields by name. */
interface EncodedHostEnum {
  readonly kind: "enum";
  readonly name: string;
  readonly variant: string;
  readonly fields: readonly { readonly name: string; readonly value: EncodedHostValue }[];
}

type EncodedHostValue =
  | EncodedHostData
  | EncodedHostEnum
  | EncodedHostOptional
  | EncodedHostResult
  | EncodedHostScalar
  | EncodedHostSequence;

type HostSuspensionValue = number | bigint | string;

/**
 * A host call's argument: a scalar or a string, the strings of a
 * `List[string]`, or a structural value in the view a host result takes.
 */
export type HostArgumentValue = HostBoundaryValue;

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
  readonly arguments: readonly HostArgumentValue[];
  readonly functionCodeId: string;
  readonly functionIndex: number;
  readonly functionName: string;
  readonly methodName: string;
  readonly provider: unknown;
  readonly providerKey: string;
  /** The qualified name of a std trait, such as `std.host.Args`, whatever the program calls it. */
  readonly standardName?: string;
  readonly resultType: ValueType;
  readonly siteId: string;
  /** The method is a suspending (`!`) method, so its call may stay pending. */
  readonly suspending: boolean;
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
  arguments: HostArgumentValue[];
}

interface HostCallState extends ArgumentBuffers {
  readonly call: MutableHostSuspensionCall;
  outcome?: HostSuspensionOutcome;
  resultNode?: HostBoundaryNode;
  resultBytes?: Uint8Array;
}

interface HostString {
  readonly bytes: Uint8Array;
}

type HostImport = (...arguments_: unknown[]) => unknown;

interface InstantiateOptions {
  readonly console?: (text: string, provider: unknown) => void;
  readonly consoleError?: (text: string, provider: unknown) => void;
  readonly entryError?: (report: string, provider: unknown) => void; // else consoleError
  readonly trace?: (functionIndex: number, event: SuspensionTraceEvent) => void;
  readonly pending?: (functionIndex: number, pollCount: number) => boolean;
  readonly record?: (event: ReplayEvent) => void;
  readonly replay?: readonly ReplayEvent[];
  readonly providerConfigurationId?: string;
  readonly hostCapabilities?: readonly string[];
  readonly needs?: (traits: readonly string[]) => void; // host-boundary.ts checkedModule
  readonly hostSuspensionCancel?: (call: HostSuspensionCall) => void;
  readonly hostSuspensionInvoke?: (call: HostSuspensionCall) => HostSuspensionOutcome;
  readonly hostSuspensionPending?: (call: HostSuspensionCall) => boolean;
  /** Host functions that override or add to `HOST_FUNCTIONS`. */
  readonly hostFunctions?: Readonly<Record<string, HostFunction>>;
  readonly parse?: ParseOptions;
  /** A release build (CompileOptions.release). */
  readonly release?: boolean;
  /** An integration test program (CompileOptions.integrationTest). */
  readonly integrationTest?: boolean;
  /** A doc test's program (CompileOptions.docTest). */
  readonly docTest?: boolean;
  /** A test build (CompileOptions.testBuild). */
  readonly testBuild?: boolean;
  /** A single-file program's stem, which its TypeId names start with (CompileOptions.programName). */
  readonly programName?: string;
  /** A compilation of `source` to instantiate again, as for a fresh test instance. */
  readonly compilation?: Compilation;
  /** Where a `dbg` line names its call (CompileOptions.debugLocation). */
  readonly debugLocation?: CompileOptions["debugLocation"];
  /**
   * Receives each `dbg` line, the program's debug output
   * (spec/lang/10-modules.md#r-module.dbg.stream); `consoleError` without it.
   */
  readonly debugOutput?: (line: string) => void;
}

interface Instantiation {
  readonly compilation: Compilation;
  readonly instance: WebAssembly.Instance;
  readonly replay: ReplaySession;
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

// The types that consent to come in from a host, by program
// (module.boundary.consent.in).
const incomingConsent = new WeakMap<BoundaryTypes, ReadonlySet<string>>();

function consentsToComeIn(program: BoundaryTypes): ReadonlySet<string> {
  let found = incomingConsent.get(program);
  if (!found) {
    found = new Set(program.consentIn);
    incomingConsent.set(program, found);
  }
  return found;
}

function checkedHostBoundaryNode(
  program: BoundaryTypes,
  type: ValueType,
  value: HostBoundaryValue,
  ancestors: Set<object> = new Set(),
): HostBoundaryNode {
  const shape = boundaryShape(type, ...programLookups(program));
  if (shape.kind === "scalar" || shape.kind === "string") {
    if (typeof value === "object") throw new Error(`expected a ${type} scalar`);
    return { kind: "scalar", type, value: checkedHostValue(type, value) };
  }
  if (shape.kind === "optional")
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
        children: [checkedHostBoundaryNode(program, shape.inner, tagged.value!, ancestors)],
      };
    });
  if (shape.kind === "tuple")
    return withBoundaryObject(value, ancestors, () => {
      if (!Array.isArray(value) || value.length !== shape.elements.length)
        throw new Error(`expected '${type}' as an array of ${shape.elements.length} values`);
      return {
        kind: "sequence",
        children: shape.elements.map((element, index) =>
          checkedHostBoundaryNode(program, element, value[index]!, ancestors),
        ),
      };
    });
  if (shape.kind === "list")
    return withBoundaryObject(value, ancestors, () => {
      if (!Array.isArray(value)) throw new Error(`expected '${type}' as an array`);
      return {
        kind: "sequence",
        children: value.map((element) =>
          checkedHostBoundaryNode(program, shape.element, element, ancestors),
        ),
      };
    });
  if (shape.kind === "data") {
    const data = shape.declaration;
    const dataName = data.name;
    if (!boundaryFieldsVisible(data, consentsToComeIn(program)))
      throw new Error(`host '${dataName}' cannot set a private field`);
    // A newtype, as `Path`, crosses as its base value.
    if (data.newtype)
      return {
        kind: "record",
        children: [checkedHostBoundaryNode(program, data.fields[0]!.type, value, ancestors)],
      };
    return withBoundaryObject(value, ancestors, () => {
      const object = hostObject(value);
      const expected = new Set(data.fields.map((field) => field.name));
      const extra = Object.keys(object).find((name) => !expected.has(name));
      if (extra) throw new Error(`host '${dataName}' has an unknown field '${extra}'`);
      const substitutions = new Map(
        data.genericParameters.map(
          (parameter, index) => [parameter, shape.arguments[index]!] as const,
        ),
      );
      return {
        kind: "record",
        children: data.fields.map((field) => {
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
  }
  if (shape.kind === "result")
    return withBoundaryObject(value, ancestors, () => {
      // `{ tag: "ok" | "err", value? }`, as a scalar-side result crosses.
      const tagged = hostObject(value);
      if (tagged.tag !== "ok" && tagged.tag !== "err")
        throw new Error(`expected '${type}' as { tag: "ok" | "err", value? }`);
      const side = tagged.tag === "ok" ? shape.ok : shape.err;
      const tag = tagged.tag === "ok" ? 0 : 1;
      if (side === "void") {
        if (Object.keys(tagged).length !== 1)
          throw new Error(`'${type}' '${tagged.tag}' has no payload`);
        return { kind: "variant", tag, children: [] };
      }
      if (!("value" in tagged) || Object.keys(tagged).length !== 2)
        throw new Error(`expected '${type}' '${tagged.tag}' with a value`);
      return {
        kind: "variant",
        tag,
        children: [checkedHostBoundaryNode(program, side, tagged.value!, ancestors)],
      };
    });
  if (shape.kind === "enum") {
    const enumeration = shape.declaration;
    return withBoundaryObject(value, ancestors, () => {
      // `{ tag: "Variant", field: value, ... }`, with the variant's fields by name.
      const object = hostObject(value);
      const variant = enumeration.variants.find(({ name }) => name === object.tag);
      if (!variant)
        throw new Error(
          `expected '${type}' as { tag: one of ${enumeration.variants.map(({ name }) => `"${name}"`).join(", ")} }`,
        );
      const expected = new Set(["tag", ...variant.fields.map((field) => field.name)]);
      const extra = Object.keys(object).find((name) => !expected.has(name));
      if (extra) throw new Error(`host '${type}.${variant.name}' has an unknown field '${extra}'`);
      return {
        kind: "variant",
        tag: variant.tag,
        children: variant.fields.map((field) => {
          if (!(field.name in object))
            throw new Error(`host '${type}.${variant.name}' is missing field '${field.name}'`);
          return checkedHostBoundaryNode(program, field.type, object[field.name]!, ancestors);
        }),
      };
    });
  }
  throw new Error(`the host cannot build a '${type}' boundary value`);
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
  if (side !== "string" && !isBoundaryScalar(side))
    throw new Error(`the host cannot build a '${side}' for ${type}`);
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
  program: BoundaryTypes,
  type: ValueType,
  node: HostBoundaryNode,
): EncodedHostValue {
  const shape = boundaryShape(type, ...programLookups(program));
  if (shape.kind === "scalar" || shape.kind === "string") {
    if (node.kind !== "scalar") throw new Error(`host boundary node does not match '${type}'`);
    return encodeHostValue(type, node.value);
  }
  if (shape.kind === "optional") {
    if (node.kind !== "variant" || (node.tag !== 0 && node.tag !== 1))
      throw new Error(`host boundary node does not match '${type}'`);
    return node.tag === 0
      ? { kind: "none" }
      : {
          kind: "some",
          value: encodeHostBoundaryNode(program, shape.inner, node.children[0]!),
        };
  }
  if (shape.kind === "tuple") {
    if (node.kind !== "sequence" || node.children.length !== shape.elements.length)
      throw new Error(`host boundary node does not match '${type}'`);
    return {
      kind: "tuple",
      values: shape.elements.map((element, index) =>
        encodeHostBoundaryNode(program, element, node.children[index]!),
      ),
    };
  }
  if (shape.kind === "list") {
    if (node.kind !== "sequence") throw new Error(`host boundary node does not match '${type}'`);
    return {
      kind: "list",
      values: node.children.map((child) => encodeHostBoundaryNode(program, shape.element, child)),
    };
  }
  if (shape.kind === "data") {
    const data = shape.declaration;
    if (node.kind !== "record" || node.children.length !== data.fields.length)
      throw new Error(`host boundary node does not match '${type}'`);
    const substitutions = new Map(
      data.genericParameters.map(
        (parameter, index) => [parameter, shape.arguments[index]!] as const,
      ),
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
  if (shape.kind === "result") {
    if (node.kind !== "variant" || (node.tag !== 0 && node.tag !== 1))
      throw new Error(`host boundary node does not match '${type}'`);
    const side = node.tag === 0 ? shape.ok : shape.err;
    const kind = node.tag === 0 ? "ok" : "err";
    return side === "void"
      ? { kind }
      : { kind, value: encodeHostBoundaryNode(program, side, node.children[0]!) };
  }
  if (shape.kind === "enum") {
    const enumeration = shape.declaration;
    const variant = enumeration.variants.find(
      ({ tag }) => node.kind === "variant" && tag === node.tag,
    );
    if (node.kind !== "variant" || !variant || node.children.length !== variant.fields.length)
      throw new Error(`host boundary node does not match '${type}'`);
    return {
      kind: "enum",
      name: enumeration.standardName ?? enumeration.name,
      variant: variant.name,
      fields: variant.fields.map((field, index) => ({
        name: field.name,
        value: encodeHostBoundaryNode(program, field.type, node.children[index]!),
      })),
    };
  }
  throw new Error(`host boundary node does not match '${type}'`);
}

function decodeHostBoundaryNode(
  program: BoundaryTypes,
  type: ValueType,
  encoded: EncodedHostValue,
): HostBoundaryNode {
  const shape = boundaryShape(type, ...programLookups(program));
  if (shape.kind === "scalar" || shape.kind === "string")
    return { kind: "scalar", type, value: decodeHostValue(type, encoded) };
  if (shape.kind === "optional") {
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
      children: [decodeHostBoundaryNode(program, shape.inner, encoded.value)],
    };
  }
  if (shape.kind === "tuple") {
    if (
      encoded.kind !== "tuple" ||
      !Array.isArray(encoded.values) ||
      encoded.values.length !== shape.elements.length
    )
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "sequence",
      children: shape.elements.map((element, index) =>
        decodeHostBoundaryNode(program, element, encoded.values[index]!),
      ),
    };
  }
  if (shape.kind === "list") {
    if (encoded.kind !== "list" || !Array.isArray(encoded.values))
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "sequence",
      children: encoded.values.map((value) =>
        decodeHostBoundaryNode(program, shape.element, value),
      ),
    };
  }
  if (shape.kind === "data") {
    const data = shape.declaration;
    if (
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
    const substitutions = new Map(
      data.genericParameters.map(
        (parameter, index) => [parameter, shape.arguments[index]!] as const,
      ),
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
  if (shape.kind === "result") {
    if (encoded.kind !== "ok" && encoded.kind !== "err")
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    const side = encoded.kind === "ok" ? shape.ok : shape.err;
    const tag = encoded.kind === "ok" ? 0 : 1;
    if (side === "void") return { kind: "variant", tag, children: [] };
    if (encoded.value === undefined)
      throw new Error(`replay boundary type '${encoded.kind}' has no payload for '${type}'`);
    return {
      kind: "variant",
      tag,
      children: [decodeHostBoundaryNode(program, side, encoded.value)],
    };
  }
  if (shape.kind === "enum") {
    const enumeration = shape.declaration;
    const variant =
      encoded.kind === "enum" && encoded.name === (enumeration.standardName ?? enumeration.name)
        ? enumeration.variants.find(({ name }) => name === encoded.variant)
        : undefined;
    if (
      encoded.kind !== "enum" ||
      !variant ||
      encoded.fields.length !== variant.fields.length ||
      encoded.fields.some((field, index) => field.name !== variant.fields[index]!.name)
    )
      throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
    return {
      kind: "variant",
      tag: variant.tag,
      children: variant.fields.map((field, index) =>
        decodeHostBoundaryNode(program, field.type, encoded.fields[index]!.value),
      ),
    };
  }
  throw new Error(`replay boundary type '${encoded.kind}' does not match '${type}'`);
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

/**
 * A host call's argument as a replay event records it; a `List[string]` is a
 * list of strings, and a structural argument is encoded as a structural
 * host result is.
 */
function encodeHostArgument(
  program: BoundaryTypes,
  type: ValueType,
  value: HostArgumentValue,
): EncodedHostValue {
  if (structuralHostArgument(type))
    return encodeHostBoundaryNode(program, type, checkedHostBoundaryNode(program, type, value));
  if (isStringListArgument(type)) {
    if (!Array.isArray(value)) throw new Error(`expected '${type}' as an array`);
    return { kind: "list", values: value.map((element) => encodeHostValue("string", element)) };
  }
  return encodeHostValue(type, value as HostSuspensionValue);
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
  program: BoundaryTypes,
  call: HostSuspensionCall,
  method: HostMethodInterface,
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
      encodeHostArgument(program, method.parameters[index]!, argument),
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
    // `dbg` lines and fix-its quote the checked source (checker/debug-print.ts).
    const checked = check(program, { ...options, sourceText: options.sourceText ?? source });
    // Lexical warnings travel with the program; parser diagnostics are empty
    // whenever a program exists, so concatenation only adds those warnings.
    return { hir: checked.program, diagnostics: [...parsed.diagnostics, ...checked.diagnostics] };
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
  // A release build keeps its panic sites too: its remaining panics, such as
  // an index out of bounds, report where they happened.
  const sites: PanicSite[] = [];
  return {
    wat: emitWat(analysis.hir, { release: options.release, sites }),
    hir: analysis.hir,
    diagnostics: analysis.diagnostics,
    sites,
  };
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
  const { bytes, siteMap } = await assembleWat(compilation.wat);
  return { ...compilation, bytes, ...(siteMap ? { siteMap } : {}) };
}

/**
 * The imports that read a ready host capability result (emitter/host-providers.ts):
 * a scalar, a string's UTF-8 bytes, or a `Result[T, E]`'s tag and payload.
 */
function hostResultImports(
  program: BoundaryTypes,
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
  // The UTF-8 bytes of a string node, encoded once and cached by node: the
  // Wasm side reads one byte per call, so re-encoding per byte is quadratic.
  const stringBytes = new WeakMap<object, Uint8Array>();
  const boundaryStringBytes = (value: unknown): Uint8Array => {
    const node = boundaryNode(value);
    if (node.kind !== "scalar" || typeof node.value !== "string")
      throw new Error("host boundary node is not a string scalar");
    let bytes = stringBytes.get(node);
    if (!bytes) {
      bytes = textEncoder.encode(node.value);
      stringBytes.set(node, bytes);
    }
    return bytes;
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
    host_boundary_string_length: (value) => boundaryStringBytes(value).length,
    host_boundary_string_byte: (value, index) => boundaryStringBytes(value)[Number(index)],
  };
}

function makeHostCall(
  functionIdentity: (index: number) => FunctionIdentity | undefined,
  provider: unknown,
  trait: Pick<HirTrait, "name" | "standardName">,
  methodName: string,
  resultType: ValueType,
  suspending: boolean,
  functionIndex: number,
  siteOffset: number,
  arguments_: HostArgumentValue[],
): MutableHostSuspensionCall {
  const identity = functionIdentity(functionIndex);
  if (!identity) throw new Error(`host provider call has unknown function index ${functionIndex}`);
  const providerKey = trait.name;
  return {
    ...(trait.standardName ? { standardName: trait.standardName } : {}),
    arguments: arguments_,
    functionCodeId: identity.codeId,
    functionIndex: identity.index,
    functionName: identity.name,
    methodName,
    provider,
    providerKey,
    resultType,
    siteId: `${identity.name}:provider:${providerKey}.${methodName}:${siteOffset - identity.start}`,
    suspending,
  };
}

/**
 * The `host:NAME` import of each `lib/std` function whose `@intrinsic` names a
 * host function (host-functions.ts). `dbg_write` writes to the program's
 * debug output, apart from its console (spec/lang/10-modules.md#r-module.dbg.stream).
 */
function hostFunctionImports(
  runtime: RuntimeInterface,
  options: InstantiateOptions,
  textDecoder: TextDecoder,
): Record<string, HostImport> {
  const textEncoder = new TextEncoder();
  const debugOutput =
    options.debugOutput ?? ((line: string) => options.consoleError?.(line, undefined));
  const hostFunctions: Readonly<Record<string, HostFunction>> = {
    ...HOST_FUNCTIONS,
    dbg_write: (line) => debugOutput(String(line)),
    ...options.hostFunctions,
  };
  const imports: Record<string, HostImport> = {};
  for (const declaration of runtime.hostFunctions) {
    const { name } = declaration;
    imports[`host:${name}`] = (...arguments_) => {
      const implementation = hostFunctions[name];
      if (!implementation) throw new Error(`the host has no function '${name}'`);
      const result = implementation(
        ...declaration.parameters.map((parameter, index) =>
          parameter === "string"
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
  return imports;
}

/**
 * A module's import that the host does not provide, so `hd build` did not
 * write the module (spec/cli/command-line.md#r-cli.wasm.not-hd.detect).
 */
export class UnknownImportError extends Error {
  constructor(importName: string) {
    super(`it imports ${importName}, which this hd does not provide`);
  }
}

/** Compiles `source`, or takes `options.compilation`, and instantiates its module. */
export async function instantiate(
  source: string,
  options: InstantiateOptions = {},
): Promise<Instantiation> {
  const compilation = options.compilation ?? (await compileToWasm(source, options));
  const { instance, replay } = await instantiateModule(
    compilation.bytes,
    runtimeInterface(source, compilation.hir),
    options,
    compilation,
  );
  return { compilation, instance, replay };
}

/**
 * Instantiates the Wasm module `input` with the host imports that `runtime`
 * describes: a module compiled just now, or one `hd build` wrote
 * (runtime-interface.ts). `artifact` holds a compilation's panic sites,
 * which a built module lacks.
 */
export async function instantiateModule(
  input: Uint8Array | WebAssembly.Module,
  runtime: RuntimeInterface,
  options: InstantiateOptions = {},
  artifact: Parameters<typeof panicLocator>[0] = {},
): Promise<Omit<Instantiation, "compilation">> {
  const boundary = runtime.boundary;
  const hostEnums = boundary.enums;
  const functionIdentities = runtime.functions;
  const functionIdentity = (index: number): FunctionIdentity | undefined =>
    functionIdentities.find((identity) => identity.index === index);
  const configurationId = options.providerConfigurationId ?? "default";
  const textEncoder = new TextEncoder();
  // Strings are UTF-8 at every host boundary, so a leading U+FEFF is text, not
  // a byte order mark to drop.
  const textDecoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
  // A panic that an import raises names the program operation it happened at.
  const located = panicLocator(artifact, options.debugLocation);
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
  // The module initializer's entry requirements, bound the way `main!` rows
  // are. Only the run path instantiates, so check-time behavior is untouched.
  const requirements = runtime.initializerRequirements;
  if (requirements.length > 0)
    hostImports.init_provider = (index) => ({ requirement: requirements[Number(index)]! });
  hostImports.panic_with_message = located((code, message) => {
    const bytes = (message as HostString).bytes;
    throw new RuntimePanicError(runtimePanicName(Number(code)), textDecoder.decode(bytes));
  });
  Object.assign(hostImports, structuralBoundaryImports(textEncoder), argumentTokenImports());
  // Each host capability trait, with the methods the module calls.
  for (const trait of runtime.hostTraits) {
    // An unrecorded provider's calls are neither recorded nor replayed
    // (host-functions.ts). A profile may hold even a built-in call pending;
    // once released, the built-in supplies the ready answer.
    const recorded = !UNRECORDED_PROVIDERS.has(trait.name);
    for (const method of trait.methods) {
      const prefix = `host_${trait.index}_${method.index}`;
      const builtIn = HOST_PROVIDERS[`${trait.name}.${method.name}`];
      hostImports[`${prefix}_begin`] = (provider, functionIndex, siteOffset, ...arguments_) => ({
        ...argumentBuffers(method.parameters, arguments_),
        call: makeHostCall(
          functionIdentity,
          provider,
          trait,
          method.name,
          method.result,
          method.suspending,
          functionIndex as number,
          siteOffset as number,
          method.parameters.map((parameter, index) =>
            parameter === "string"
              ? ""
              : structuralHostArgument(parameter)
                ? []
                : hostArgumentValue(
                    parameter,
                    numericType(parameter)?.wasm === "i64"
                      ? (arguments_[index] as bigint)
                      : Number(arguments_[index]),
                  ),
          ),
        ),
      });
      Object.assign(
        hostImports,
        streamedArgumentImports(prefix, `${trait.name}.${method.name}`, method.parameters),
      );
      hostImports[`${prefix}_poll`] = located((value) => {
        const state = value as HostCallState;
        const { call } = state;
        decodeStreamedArguments(boundary, method.parameters, state, call.arguments, textDecoder);
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
                  encodeHostArgument(boundary, method.parameters[index]!, call.arguments[index]!),
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
            ...(expected.encodedValue && !structuralHostResult(boundary, method.result)
              ? {
                  value: decodeHostResult(method.result, expected.encodedValue, hostEnums),
                }
              : {}),
          };
          if (
            !state.outcome.pending &&
            expected.encodedValue &&
            structuralHostResult(boundary, method.result)
          )
            state.resultNode = decodeHostBoundaryNode(
              boundary,
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
          const value = structuralHostResult(boundary, method.result)
            ? (() => {
                if (state.outcome!.value === undefined)
                  throw new RuntimePanicError(
                    "host-contract",
                    `host provider ${trait.name}.${method.name} broke its contract: returned no boundary result`,
                  );
                try {
                  state.resultNode = checkedHostBoundaryNode(
                    boundary,
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
          boundary,
          call,
          method,
          state.outcome,
          configurationId,
          hostEnums,
        );
        if (recorded) options.record?.(event);
        return state.outcome.pending ? 0 : 1;
      });
      hostImports[`${prefix}_cancel`] = (value) => {
        if (!builtIn) options.hostSuspensionCancel?.((value as HostCallState).call);
      };
      Object.assign(
        hostImports,
        hostResultImports(boundary, prefix, `${trait.name}.${method.name}`, method.result),
      );
    }
  }
  // The generic host-function boundary, and an entry `.Err` report (host-functions.ts).
  Object.assign(hostImports, hostStringImports(options.entryError ?? options.consoleError));
  // A host function may panic, as `lib/std`'s panic primitive does.
  for (const [name, host] of Object.entries(hostFunctionImports(runtime, options, textDecoder)))
    hostImports[name] = located(host);
  const imports: WebAssembly.ModuleImports = {
    ...hostImports,
    trace: options.trace ?? (() => undefined),
    pending,
    pow_f64: Math.pow,
    // JavaScript `%` on numbers is the truncated remainder of C `fmod`.
    rem_f64: (left: number, right: number) => left % right,
    panic: located((code) => {
      throw new RuntimePanicError(runtimePanicName(Number(code)));
    }),
  };
  const module =
    input instanceof WebAssembly.Module ? input : await WebAssembly.compile(input as BufferSource);
  // Every import is one this host provides, or `hd build` did not write the
  // module (spec/cli/command-line.md#r-cli.wasm.not-hd.detect).
  const unknown = WebAssembly.Module.imports(module).find(
    (entry) => entry.module !== "hd" || !Object.hasOwn(imports, entry.name),
  );
  if (unknown) throw new UnknownImportError(`${unknown.module}.${unknown.name}`);
  await checkedModule(module, runtime.hostTraits, options.needs);
  const instance = await WebAssembly.instantiate(module, { hd: imports });
  // Every caller (`hd run`, `hd test`, the REPL, the playground) reads the wrapped exports.
  Object.defineProperty(instance, "exports", { value: stackExhaustionPanics(instance.exports) });
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
  return { instance, replay };
}
