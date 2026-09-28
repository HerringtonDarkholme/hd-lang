import type { HirGenericBound, HirTrait, HirTraitImplementation, ValueType } from "../hir.ts";
import {
  nominalGenericParts,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";
import { genericTypeName, matchTraitImplementation } from "./shared.ts";

// `std.format.Debug` (spec/09-traits.md#debug-trait): `std` implements it for
// the primitives, collections, optionals, `Result`, and tuples whose type
// arguments implement it; any other type needs an `impl` or `@derive(Debug)`.
const PRIMITIVES = new Set(["i32", "i64", "u8", "f64", "bool", "char", "string"]);

/** Whether `std` supplies `Debug` for `type` without a source `impl`. */
export function builtinDebug(type: ValueType): boolean {
  const target = readonlyType(type);
  if (PRIMITIVES.has(target)) return true;
  const tuple = tupleParts(target);
  if (tuple !== undefined) return true;
  if (optionalInner(target) !== undefined || resultParts(target)) return true;
  const name = nominalGenericParts(target)?.name;
  return name === "List" || name === "Map";
}

/** Whether `type` implements `Debug`, given the generic bounds in scope. */
export function implementsDebug(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
  bounds: readonly HirGenericBound[],
): boolean {
  const target = readonlyType(type);
  const recurse = (inner: ValueType): boolean =>
    implementsDebug(inner, traitTypes, implementations, bounds);
  const generic = genericTypeName(target);
  if (generic)
    return bounds.some((bound) => bound.parameter === generic && bound.traitName === "Debug");
  if (PRIMITIVES.has(target)) return true;
  const tuple = tupleParts(target);
  if (tuple !== undefined) return tuple.every(recurse);
  const optional = optionalInner(target);
  if (optional !== undefined) return recurse(optional);
  const result = resultParts(target);
  if (result) return recurse(result.ok) && recurse(result.error);
  const nominal = nominalGenericParts(target);
  if (nominal?.name === "List" || nominal?.name === "Map") return nominal.arguments.every(recurse);
  const debug = traitTypes.get("Debug");
  return (
    debug !== undefined &&
    implementations.some((implementation) =>
      Boolean(matchTraitImplementation(implementation, debug.index, target, [])),
    ) &&
    (nominal?.arguments ?? []).every(recurse)
  );
}
