import type { HirBuiltinTraitImplementation, HirExpression, HirTrait, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";
import { PRELUDE_ORIGINS } from "./prelude-names.ts";
import { INSPECTABLE, STANDARD_DOWNCAST_VAL } from "./standard-traits.ts";

// Runtime type identity (spec/lang/09-traits.md#runtime-type-identity). A type's
// key is its canonical printable name with the outer `mut` removed; a `mut`
// inside a type argument is kept (Inspectable decision 16). A std
// declaration outside the prelude is spelled by its qualified name, as
// `std.error.Error` (r-trait.typeid.name.qualified). A package declaration
// is spelled by its package name and module path, as `acme_shop.model.User`,
// and a single-file program's by its file stem
// (r-trait.typeid.name.package, r-trait.typeid.name.single-file), so two
// declarations of different modules never share a key
// (r-trait.identity.modules). A key part
// `{ generic }` stands for a bounded type parameter whose name the bound's
// dictionary supplies at run time.
type InspectKeyPart = string | { readonly generic: string };

export interface InspectEnvironment {
  /**
   * The key spelling of a module-level data or enum declaration, from
   * `printedName`, or undefined for any other name.
   */
  readonly nominal: (name: string) => string | undefined;
  /** The key spelling of a trait, from `printedName`; the default is its name. */
  readonly trait?: (name: string) => string;
  /** A type parameter bounded directly by `Inspectable`. */
  readonly inspectableParameter: (name: string) => boolean;
  /**
   * The type whose key a handle supplies at run time, as the
   * `{ generic: HANDLE_TYPE }` part (annot.handle.fact.key).
   */
  readonly handleType?: ValueType;
  /**
   * Name any type: a function type or an unbounded type parameter by its
   * text. Only a fact list and a handle's own type key use it, since the
   * prototype's facts hold `Inspectable` values rather than `Any`.
   */
  readonly anyType?: boolean;
}

/** The key part that stands for a handle's own `F` (annot.handle.fact.key). */
export const HANDLE_TYPE = "$handle";

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
  "usize",
  "f32",
  "f64",
  "string",
]);

/**
 * Whether the module, or a std module it joins, such as `std.error`, imported
 * the standard runtime type identity surface.
 */
export function usesStandardInspect(imports: ReadonlyMap<string, string>): boolean {
  return [...imports.values()].some((imported) => imported.startsWith("std.inspect."));
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

/**
 * How a declaration named `name` in the checked program prints in a key: a
 * std declaration outside the prelude by its qualified `standardName`
 * (spec/lang/09-traits.md#r-trait.typeid.name.qualified), a package or
 * single-file declaration by its `typeIdName`
 * (spec/lang/09-traits.md#r-trait.typeid.name.package), and any other by
 * `name`.
 */
export function printedName(
  name: string,
  declaration: { readonly standardName?: string; readonly typeIdName?: string } | undefined,
): string {
  if (declaration?.typeIdName !== undefined) return declaration.typeIdName;
  const standardName = declaration?.standardName;
  if (standardName === undefined) return name;
  const dot = standardName.lastIndexOf(".");
  const short = standardName.slice(dot + 1);
  return PRELUDE_ORIGINS.get(short) === standardName.slice(0, dot) ? short : standardName;
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
  if (environment.handleType !== undefined && type === environment.handleType)
    return [{ generic: HANDLE_TYPE }];
  const mutable = mutableInner(type);
  if (mutable !== undefined) {
    const inner = inspectKey(mutable, environment, argument, nested);
    return nested && inner
      ? optionalInner(mutable) !== undefined
        ? ["mut (", ...inner, ")"]
        : ["mut ", ...inner]
      : inner;
  }
  if (PRIMITIVES.has(type)) return [type];
  if (type === "void") return argument || environment.anyType ? ["void"] : undefined;
  if (type.startsWith("trait:") && !type.endsWith("?")) {
    if (!argument && !environment.anyType) return undefined;
    const traitKey = type.slice("trait:".length);
    const spell = environment.trait ?? ((name: string) => name);
    const application = nominalGenericParts(traitKey);
    if (!application) return [spell(traitKey)];
    const arguments_ = application.arguments.map((item) =>
      inspectKey(item, environment, true, true),
    );
    if (arguments_.some((item) => item === undefined)) return undefined;
    return [`${spell(application.name)}[`, ...join(arguments_ as InspectKeyPart[][], ", "), "]"];
  }
  const generic = /^generic:([^?[\](),]+)$/.exec(type)?.[1];
  if (generic) {
    if (environment.inspectableParameter(generic)) return [{ generic }];
    return environment.anyType ? [generic] : undefined;
  }
  if (functionParts(type)) return environment.anyType ? [type] : undefined;
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    const elements = tuple.map((element) => inspectKey(element, environment, false, true));
    if (elements.some((element) => element === undefined)) return undefined;
    return ["(", ...join(elements as InspectKeyPart[][], ", "), ")"];
  }
  const optional = optionalInner(type);
  if (optional !== undefined) {
    const inner = inspectKey(optional, environment, true, true);
    return (
      inner &&
      (mutableInner(optional) !== undefined || functionParts(optional)
        ? ["(", ...inner, ")?"]
        : [...inner, "?"])
    );
  }
  const result = resultParts(type);
  if (result) {
    const ok = inspectKey(result.ok, environment, true, true);
    const error = inspectKey(result.error, environment, true, true);
    return ok && error ? ["Result[", ...ok, ", ", ...error, "]"] : undefined;
  }
  const nominal = nominalGenericParts(type);
  if (nominal) {
    const name =
      nominal.name === "List" || nominal.name === "Map"
        ? nominal.name
        : environment.nominal(nominal.name);
    if (name === undefined) return undefined;
    const arguments_ = nominal.arguments.map((item) => inspectKey(item, environment, true, true));
    if (arguments_.some((item) => item === undefined)) return undefined;
    return [`${name}[`, ...join(arguments_ as InspectKeyPart[][], ", "), "]"];
  }
  const name = environment.nominal(type);
  return name === undefined ? undefined : [name];
}

/**
 * The builtin Inspectable dictionary of a type with key `parts`, and the
 * bound dictionaries its `{ generic }` parts read, each from `bound`.
 */
export function inspectableBuiltin(
  parts: readonly InspectKeyPart[],
  traitIndex: number,
  targetType: ValueType,
  bound: (generic: string) => HirExpression,
): { readonly builtin: HirBuiltinTraitImplementation; readonly bounds: HirExpression[] } {
  const bounds: HirExpression[] = [];
  const positions = new Map<string, number>();
  const key = compactKey(parts).map((part) => {
    if (typeof part === "string") return part;
    let position = positions.get(part.generic);
    if (position === undefined) {
      position = bounds.length;
      positions.set(part.generic, position);
      bounds.push(bound(part.generic));
    }
    return { bound: position };
  });
  const builtin: HirBuiltinTraitImplementation = {
    kind: "inspectable",
    traitIndex,
    targetType: readonlyType(targetType),
    key,
    ...(mutableInner(targetType) !== undefined ? { outerMut: true as const } : {}),
  };
  return { builtin, bounds };
}

/** Merges adjacent literal parts. */
function compactKey(parts: readonly InspectKeyPart[]): InspectKeyPart[] {
  const result: InspectKeyPart[] = [];
  for (const part of parts) {
    const last = result.at(-1);
    if (typeof part === "string" && typeof last === "string")
      result[result.length - 1] = last + part;
    else result.push(part);
  }
  return result;
}
