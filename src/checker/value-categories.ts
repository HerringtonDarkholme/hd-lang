import type { HirData, HirEnum, HirTrait, ValueType } from "../hir.ts";
import {
  contextKeys,
  functionParts,
  nominalGenericParts,
  optionalInner,
  PRIMITIVE_TYPES,
  readonlyType,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import { genericTypeName, substituteGenericType } from "./shared.ts";

export type ValueCategory = "AnyVal" | "AnyRef";

/** Whether a trait's ordinary or category supertraits imply a sealed value category. */
export function traitImpliesValueCategory(
  trait: HirTrait,
  traitTypes: ReadonlyMap<string, HirTrait>,
  category: ValueCategory,
  seen: ReadonlySet<number> = new Set(),
): boolean {
  if (seen.has(trait.index)) return false;
  if (trait.categorySupertraits?.includes(category)) return true;
  const next = new Set([...seen, trait.index]);
  return trait.supertraits.some((supertrait) => {
    const parent = traitTypes.get(supertrait.traitName);
    return parent !== undefined && traitImpliesValueCategory(parent, traitTypes, category, next);
  });
}

export interface ValueCategoryEnvironment {
  readonly dataTypes: ReadonlyMap<string, HirData>;
  readonly enumTypes: ReadonlyMap<string, HirEnum>;
  readonly referenceParameters?: ReadonlySet<string>;
  readonly valueParameters?: ReadonlySet<string>;
}

/** Whether a resolved type satisfies one sealed value-category bound. */
export function typeSatisfiesValueCategory(
  source: ValueType,
  category: ValueCategory,
  environment: ValueCategoryEnvironment,
  seen: ReadonlySet<ValueType> = new Set(),
): boolean {
  const type = readonlyType(source);
  if (seen.has(type) || type === "never") return false;
  const next = new Set([...seen, type]);
  const generic = genericTypeName(type);
  if (generic)
    return category === "AnyRef"
      ? (environment.referenceParameters?.has(generic) ?? false)
      : (environment.valueParameters?.has(generic) ?? false);

  const nominal = nominalGenericParts(type);
  const name = nominal?.name ?? type;
  const data = environment.dataTypes.get(name);
  if (data?.newtype && data.fields[0]) {
    const arguments_ = nominal?.arguments ?? [];
    const substitutions = new Map(
      data.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
    );
    const base = substituteGenericType(data.fields[0].type, substitutions);
    return typeSatisfiesValueCategory(base, category, environment, next);
  }

  const reference =
    type.startsWith("trait:") ||
    optionalInner(type) !== undefined ||
    functionParts(type) !== undefined ||
    storedSuspensionParts(type) !== undefined ||
    suspensionParts(type) !== undefined ||
    traitSuspensionParts(type) !== undefined ||
    contextKeys(type) !== undefined ||
    name === "List" ||
    name === "Map" ||
    environment.dataTypes.has(name) ||
    environment.enumTypes.has(name);
  if (reference) return category === "AnyRef";
  const value = type === "void" || PRIMITIVE_TYPES.has(type) || tupleParts(type) !== undefined;
  return value && category === "AnyVal";
}
