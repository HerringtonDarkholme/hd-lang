// What running a built module needs to know of its program beside its Wasm
// code: the host capability methods it imports and their boundary types, the
// `@intrinsic` host functions, the identities of its functions for host call
// sites, and its entry point. `instantiate` (compiler.ts) binds a module's
// imports from it. `hd build` writes it into each module as the custom section
// `hd.runtime`, so `hd FILE.wasm` binds a module whose source it never sees
// (spec/cli/command-line.md#prebuilt-modules). The spec leaves the runtime ABI
// open (spec/lang/10-modules.md#r-module.host.abi); this section is the
// prototype's own.

import { createHash } from "node:crypto";

import { physicalSpan, sourceDocument } from "./diagnostics.ts";
import { isRuntimePrimitive } from "./emitter/index.ts";
import { emissionReachability, traitMethodKey } from "./emitter/reachability.ts";
import {
  CONSENT_TRAITS,
  consentingTypes,
  programImplementations,
  type BoundaryTypes,
} from "./host-boundary.ts";
import type { HirData, HirEnum, HirProgram, HirTraitMethod, ValueType } from "./hir.ts";

/** A function's identity at a host call site or a poll, for replay. */
export interface FunctionIdentity {
  readonly codeId: string;
  readonly end: number;
  readonly index: number;
  readonly name: string;
  readonly start: number;
}

/** A host capability method that the module calls. */
export type HostMethodInterface = Pick<
  HirTraitMethod,
  "index" | "name" | "parameters" | "result" | "suspending"
>;

/** A host capability trait, with only the methods the module calls. */
export interface HostTraitInterface {
  readonly index: number;
  readonly name: string;
  /** The qualified name of a std trait, such as `std.console.Console`. */
  readonly standardName?: string;
  readonly methods: readonly HostMethodInterface[];
}

/** A `lib/std` function whose `@intrinsic` names a host function (host-functions.ts). */
export interface HostFunctionInterface {
  readonly name: string;
  readonly parameters: readonly ValueType[];
  readonly result: ValueType;
}

/** The exported entry point: `main`, or the empty `main` of a script. */
export interface EntryInterface {
  readonly name: string;
  readonly requirements: readonly string[];
}

export interface RuntimeInterface {
  /** The data and enum types that cross the host boundary, and which consent to come in. */
  readonly boundary: BoundaryTypes;
  readonly hostTraits: readonly HostTraitInterface[];
  readonly hostFunctions: readonly HostFunctionInterface[];
  readonly functions: readonly FunctionIdentity[];
  /** The module initializer's entry requirements, bound as `main!` rows are. */
  readonly initializerRequirements: readonly string[];
  readonly entry?: EntryInterface;
}

/** The custom section that holds a built module's {@link RuntimeInterface}. */
export const RUNTIME_SECTION = "hd.runtime";

/** The layout of the section's JSON; a module with another layout is not this hd's. */
const RUNTIME_FORMAT = 1;

function functionIdentities(source: string, program: HirProgram): FunctionIdentity[] {
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

/**
 * The module initializer's entry requirements, when the initializer takes
 * providers: each arrives as the `{requirement}` stub the host passes
 * `main!` rows. A suspending initializer keeps its driver path.
 */
function initializerEntryRequirements(hir: HirProgram): readonly string[] {
  const initializer = hir.functions.find((declaration) => declaration.index === hir.initializer);
  return initializer !== undefined && !initializer.suspending ? initializer.requirements : [];
}

/**
 * The data and enum types that `roots` name, and those their fields name in
 * turn. A word of a type that names no declaration is skipped, so the walk
 * may keep more than a boundary reaches, never less.
 */
function reachableTypes(
  program: HirProgram,
  roots: readonly ValueType[],
): { data: HirData[]; enums: HirEnum[] } {
  const data = new Map<string, HirData>();
  const enums = new Map<string, HirEnum>();
  const pending = [...roots];
  for (let type = pending.pop(); type !== undefined; type = pending.pop()) {
    for (const [name] of type.matchAll(/[A-Za-z_][\w.]*/g)) {
      if (data.has(name) || enums.has(name)) continue;
      const declaration = program.data.find((item) => item.name === name);
      if (declaration) {
        data.set(name, declaration);
        pending.push(...declaration.fields.map((field) => field.type));
        continue;
      }
      const enumeration = program.enums.find((item) => item.name === name);
      if (!enumeration) continue;
      enums.set(name, enumeration);
      pending.push(
        ...enumeration.sharedFields.map((field) => field.type),
        ...enumeration.variants.flatMap((variant) => variant.fields.map((field) => field.type)),
      );
    }
  }
  return { data: [...data.values()], enums: [...enums.values()] };
}

const interfaces = new WeakMap<HirProgram, RuntimeInterface>();

/**
 * What running `program`'s module needs of it. `source` is the text that
 * spans without a source document of their own point into.
 */
export function runtimeInterface(source: string, program: HirProgram): RuntimeInterface {
  const known = interfaces.get(program);
  if (known) return known;
  const live = emissionReachability(program);
  const hostCapabilities = new Set(program.hostCapabilities);
  const hostTraits = program.traits.flatMap((trait): HostTraitInterface[] => {
    if (!hostCapabilities.has(trait.name)) return [];
    const methods = trait.methods
      .filter((method) => live.traitMethods.has(traitMethodKey(trait.index, method.index)))
      .map(({ index, name, parameters, result, suspending }) => ({
        index,
        name,
        parameters,
        result,
        suspending,
      }));
    if (methods.length === 0) return [];
    return [
      {
        index: trait.index,
        name: trait.name,
        ...(trait.standardName ? { standardName: trait.standardName } : {}),
        methods,
      },
    ];
  });
  const hostFunctions = live.program.functions.flatMap((declaration): HostFunctionInterface[] =>
    declaration.intrinsic && !isRuntimePrimitive(declaration.intrinsic)
      ? [
          {
            name: declaration.intrinsic,
            parameters: declaration.parameters.map((parameter) => parameter.type),
            result: declaration.result,
          },
        ]
      : [],
  );
  const { data, enums } = reachableTypes(
    program,
    hostTraits.flatMap((trait) =>
      trait.methods.flatMap((method) => [...method.parameters, method.result]),
    ),
  );
  const kept = new Set(data.map((item) => item.name));
  const entry = program.functions.find((declaration) => declaration.entry);
  const result: RuntimeInterface = {
    boundary: {
      data,
      enums,
      consentIn: [...consentingTypes(programImplementations(program), CONSENT_TRAITS.in)].filter(
        (name) => kept.has(name),
      ),
    },
    hostTraits,
    hostFunctions,
    functions: functionIdentities(source, live.program),
    initializerRequirements: initializerEntryRequirements(program),
    ...(entry ? { entry: { name: entry.name, requirements: entry.requirements } } : {}),
  };
  interfaces.set(program, result);
  return result;
}

/** `value` as an unsigned LEB128, as a Wasm section's sizes are written. */
function leb128(value: number): number[] {
  const bytes: number[] = [];
  do {
    let byte = value & 0x7f;
    value >>>= 7;
    if (value !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (value !== 0);
  return bytes;
}

/** `bytes` with `runtime` appended as the custom section `hd.runtime`. */
export function withRuntimeSection(bytes: Uint8Array, runtime: RuntimeInterface): Uint8Array {
  const encoder = new TextEncoder();
  const name = encoder.encode(RUNTIME_SECTION);
  // A `ReadonlyMap` field, such as `genericDefaults`, is not part of the boundary walk.
  const payload = encoder.encode(
    JSON.stringify({ format: RUNTIME_FORMAT, ...runtime }, (_key, value: unknown) =>
      value instanceof Map ? undefined : value,
    ),
  );
  const content = [...leb128(name.length), ...name];
  const size = content.length + payload.length;
  const section = new Uint8Array([0, ...leb128(size), ...content]);
  const result = new Uint8Array(bytes.length + section.length + payload.length);
  result.set(bytes);
  result.set(section, bytes.length);
  result.set(payload, bytes.length + section.length);
  return result;
}

/**
 * The {@link RuntimeInterface} that `hd build` wrote into `module`, or
 * undefined when the module has no such section in this hd's layout.
 */
export function readRuntimeSection(module: WebAssembly.Module): RuntimeInterface | undefined {
  const [section] = WebAssembly.Module.customSections(module, RUNTIME_SECTION);
  if (!section) return undefined;
  try {
    const { format, ...runtime } = JSON.parse(new TextDecoder().decode(section)) as {
      format?: unknown;
    } & RuntimeInterface;
    return format === RUNTIME_FORMAT ? runtime : undefined;
  } catch {
    return undefined;
  }
}
