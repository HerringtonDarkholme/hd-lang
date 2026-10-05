import type { Program } from "../ast.ts";
import type { ValueType } from "../hir.ts";
import { isNarrowInteger } from "../numeric.ts";
import { nominalGenericParts } from "../types.ts";

import {
  genericTypeName,
  matchGenericTypePattern,
  MAX_BOUND_DEPTH,
  resolveGenericType,
} from "./shared.ts";

// Map key types (spec/lang/04-type-system.md#map-key-types): how a map stores its
// keys, and whether a type meets the bound `Map[K < Eq & Hash, V]`.

/**
 * How a map stores and compares its keys at run time: 0 for an `i32`-like
 * scalar, 1 for a string, and 2 for any other key type, which the map
 * compares with its `Eq`. Which types may key a map is the shared
 * written-application check's (`writtenApplicationBoundProblem`).
 */
export function mapKeyKind(type: ValueType): 0 | 1 | 2 {
  if (isNarrowInteger(type) || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return 2;
}

/**
 * Whether `type` implements `trait` through an implementation, std's
 * included, whose bounds its type arguments meet, as a tuple does through
 * its tuple template's instance.
 */
export function implementsTrait(
  type: ValueType,
  trait: string,
  hashableParameters: ReadonlySet<string>,
  depth: number,
): boolean {
  const generic = genericTypeName(type);
  if (generic) return hashableParameters.has(generic) && (trait === "Eq" || trait === "Hash");
  if (depth > MAX_BOUND_DEPTH) return false;
  return (implementedTraits.get(trait) ?? []).some((implementation) => {
    if (implementation.parameters.size === 0) return implementation.target === type;
    const substitutions = new Map<string, ValueType>();
    if (!matchGenericTypePattern(implementation.pattern, type, substitutions)) return false;
    return implementation.bounds.every(({ parameter, traits }) => {
      const actual = substitutions.get(parameter);
      return (
        actual !== undefined &&
        traits.every((bound) => implementsTrait(actual, bound, hashableParameters, depth + 1))
      );
    });
  });
}

interface ImplementationPattern {
  readonly target: string;
  readonly pattern: ValueType;
  readonly parameters: ReadonlySet<string>;
  readonly bounds: readonly { readonly parameter: string; readonly traits: readonly string[] }[];
}

// Each trait's implementations, set for each checked program before its
// types resolve (checker/program.ts).
let implementedTraits: ReadonlyMap<string, readonly ImplementationPattern[]> = new Map();

export function setHashableKeyTypes(program: Program): void {
  const patterns = new Map<string, ImplementationPattern[]>();
  for (const item of program.implementations) {
    if (!item.traitName) continue;
    // Keyed by the trait's base name: the written-application check
    // compares required bounds by name, leaving arguments to call sites.
    const key = nominalGenericParts(item.traitName)?.name ?? item.traitName;
    const parameters = new Set(item.genericParameters);
    const list = patterns.get(key) ?? [];
    list.push({
      target: item.targetName,
      pattern: resolveGenericType(item.targetName, parameters),
      parameters,
      bounds: item.genericBounds,
    });
    patterns.set(key, list);
  }
  implementedTraits = patterns;
}
