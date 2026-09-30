import type { HirTrait, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  resultParts,
  tupleParts,
} from "../types.ts";
import { INSPECTABLE, STANDARD_DOWNCAST_VAL } from "./standard-traits.ts";

// Runtime type identity (spec/09-traits.md#runtime-type-identity). A type's
// key is its canonical printable name with the outer `mut` removed; a `mut`
// inside a type argument is kept (Inspectable decision 16). A key part
// `{ generic }` stands for a bounded type parameter whose name the bound's
// dictionary supplies at run time.
export type InspectKeyPart = string | { readonly generic: string };

export interface InspectEnvironment {
  /** A module-level data or enum declaration. */
  readonly nominal: (name: string) => boolean;
  /** A type parameter bounded directly by `Inspectable`. */
  readonly inspectableParameter: (name: string) => boolean;
}

const PRIMITIVES = new Set([
  "bool",
  "char",
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
  "string",
]);

/** Whether the module imported the standard runtime type identity surface. */
export function usesStandardInspect(imports: ReadonlyMap<string, string>): boolean {
  return [...imports.values()].some(
    (imported) => imported.startsWith("std.inspect.") || imported === "std.error.Error",
  );
}

export function isDowncastValImport(imports: ReadonlyMap<string, string>, name: string): boolean {
  return imports.get(name) === STANDARD_DOWNCAST_VAL;
}

/** Whether `name` is the standard `Inspectable` or has it as a transitive supertrait. */
export function extendsInspectable(
  traitTypes: ReadonlyMap<string, HirTrait>,
  name: string,
): boolean {
  const byIndex = new Map([...traitTypes.values()].map((trait) => [trait.index, trait] as const));
  const pending = [traitTypes.get(name)];
  const seen = new Set<number>();
  while (pending.length > 0) {
    const trait = pending.pop();
    if (!trait || seen.has(trait.index)) continue;
    seen.add(trait.index);
    if (trait.name === INSPECTABLE) return true;
    pending.push(...trait.supertraits.map((parent) => byIndex.get(parent.traitIndex)));
  }
  return false;
}

function join(parts: readonly (readonly InspectKeyPart[])[], separator: string): InspectKeyPart[] {
  return parts.flatMap((part, index) => (index === 0 ? [...part] : [separator, ...part]));
}

/**
 * The key parts of an inspectable type, or undefined when the type is not
 * inspectable. `argument` admits `void` and trait value types, which count
 * as inspectable only as type arguments. `nested` marks a type argument or
 * element, whose `mut` is part of the identity; the outer `mut` is not.
 */
export function inspectKey(
  type: ValueType,
  environment: InspectEnvironment,
  argument = false,
  nested = false,
): InspectKeyPart[] | undefined {
  const mutable = mutableInner(type);
  if (mutable !== undefined) {
    const inner = inspectKey(mutable, environment, argument, nested);
    return nested && inner ? ["mut ", ...inner] : inner;
  }
  if (PRIMITIVES.has(type)) return [type];
  if (type === "void") return argument ? ["void"] : undefined;
  if (type.startsWith("trait:") && !type.endsWith("?")) {
    if (!argument) return undefined;
    const traitKey = type.slice("trait:".length);
    const application = nominalGenericParts(traitKey);
    if (!application) return [traitKey];
    const arguments_ = application.arguments.map((item) =>
      inspectKey(item, environment, true, true),
    );
    if (arguments_.some((item) => item === undefined)) return undefined;
    return [`${application.name}[`, ...join(arguments_ as InspectKeyPart[][], ", "), "]"];
  }
  const generic = /^generic:([^?[\](),]+)$/.exec(type)?.[1];
  if (generic) return environment.inspectableParameter(generic) ? [{ generic }] : undefined;
  if (functionParts(type)) return undefined;
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    const elements = tuple.map((element) => inspectKey(element, environment, false, true));
    if (elements.some((element) => element === undefined)) return undefined;
    return ["(", ...join(elements as InspectKeyPart[][], ", "), ")"];
  }
  const optional = optionalInner(type);
  if (optional !== undefined) {
    const inner = inspectKey(optional, environment, true, true);
    return inner && [...inner, "?"];
  }
  const result = resultParts(type);
  if (result) {
    const ok = inspectKey(result.ok, environment, true, true);
    const error = inspectKey(result.error, environment, true, true);
    return ok && error ? ["Result[", ...ok, ", ", ...error, "]"] : undefined;
  }
  const nominal = nominalGenericParts(type);
  if (nominal) {
    if (nominal.name !== "List" && nominal.name !== "Map" && !environment.nominal(nominal.name))
      return undefined;
    const arguments_ = nominal.arguments.map((item) => inspectKey(item, environment, true, true));
    if (arguments_.some((item) => item === undefined)) return undefined;
    return [`${nominal.name}[`, ...join(arguments_ as InspectKeyPart[][], ", "), "]"];
  }
  return environment.nominal(type) ? [type] : undefined;
}

/** Merges adjacent literal parts. */
export function compactKey(parts: readonly InspectKeyPart[]): InspectKeyPart[] {
  const result: InspectKeyPart[] = [];
  for (const part of parts) {
    const last = result.at(-1);
    if (typeof part === "string" && typeof last === "string")
      result[result.length - 1] = last + part;
    else result.push(part);
  }
  return result;
}
