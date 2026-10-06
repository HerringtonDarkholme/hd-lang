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
 * its tuple template's instance. `key` is the program's trait map, under
 * which `registerHashableKeyTypes` recorded its implementations.
 */
export function implementsTrait(
  key: object,
  type: ValueType,
  trait: string,
  hashableParameters: ReadonlySet<string>,
  depth: number,
): boolean {
  const generic = genericTypeName(type);
  if (generic) return hashableParameters.has(generic) && (trait === "Eq" || trait === "Hash");
  if (depth > MAX_BOUND_DEPTH) return false;
  const implementedTraits = registered.get(key);
  // Every program check registers its trait map first (checker/program.ts).
  if (!implementedTraits) throw new Error("internal: no implementations recorded for this program");
  return (implementedTraits.get(trait) ?? []).some((implementation) => {
    if (implementation.parameters.size === 0) return implementation.target === type;
    const substitutions = new Map<string, ValueType>();
    if (!matchGenericTypePattern(implementation.pattern, type, substitutions)) return false;
    return implementation.bounds.every(({ parameter, traits }) => {
      const actual = substitutions.get(parameter);
      return (
        actual !== undefined &&
        traits.every((bound) => implementsTrait(key, actual, bound, hashableParameters, depth + 1))
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

/**
 * Each checked program's implementations by trait, recorded under its trait
 * map, an object every check of the program shares (checker/program.ts).
 */
const registered = new WeakMap<object, ReadonlyMap<string, readonly ImplementationPattern[]>>();

/** Records `program`'s implementations under `key`, before its types resolve. */
export function registerHashableKeyTypes(key: object, program: Program): void {
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
  registered.set(key, patterns);
}
