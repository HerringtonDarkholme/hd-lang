import type { Program } from "../ast.ts";
import type { ValueType } from "../hir.ts";
import { isNarrowInteger } from "../numeric.ts";
import { mutableInner, displayType } from "../types.ts";
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
 * compares with its `Eq`. Which types may key a map is `mapKeyProblem`'s.
 */
export function mapKeyKind(type: ValueType): 0 | 1 | 2 {
  if (isNarrowInteger(type) || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return 2;
}

/**
 * Why `type` may not key a map, if it may not: a `mut` key type, or a key
 * type that fails the declared bound `Map[K < Eq & Hash, V]`
 * (spec/lang/04-type-system.md#map-key-types). A type parameter meets it
 * through its own bounds, `hashableParameters`; any other type through a
 * non-generic `Eq` and `Hash` implementation, std's included.
 */
export function mapKeyProblem(
  type: ValueType,
  hashableParameters: ReadonlySet<string> = new Set(),
): { readonly code: string; readonly message: string } | undefined {
  if (mutableInner(type) !== undefined)
    return {
      code: "invalid-map-key",
      message: `a map key type must not be mut, found '${displayType(type)}'`,
    };
  const generic = genericTypeName(type);
  const missing = generic
    ? hashableParameters.has(generic)
      ? undefined
      : "Eq and Hash"
    : ["Hash", "Eq"].find((trait) => !implementsTrait(type, trait, hashableParameters, 0));
  return missing
    ? {
        code: "unsatisfied-trait-bound",
        message: `type '${displayType(type)}' does not implement ${missing}, required by the bound on 'K' of 'Map'`,
      }
    : undefined;
}

/**
 * Whether `type` implements `trait` through an implementation, std's
 * included, whose bounds its type arguments meet, as a tuple does through
 * its tuple template's instance.
 */
function implementsTrait(
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
    const parameters = new Set(item.genericParameters);
    const list = patterns.get(item.traitName) ?? [];
    list.push({
      target: item.targetName,
      pattern: resolveGenericType(item.targetName, parameters),
      parameters,
      bounds: item.genericBounds,
    });
    patterns.set(item.traitName, list);
  }
  implementedTraits = patterns;
}
