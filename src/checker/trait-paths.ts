import type { HirAssociatedBinding, HirGenericBound, HirTrait, ValueType } from "../hir.ts";
import type { ResolvedTraitPath } from "./context.ts";
import { substituteGenericType } from "./shared.ts";

// Paths from a trait to one of its transitive supertraits, as field indices
// of the supertrait lists (spec/lang/09-traits.md#supertraits).

/** A bound of `bounds` that proves a required trait: directly, or through the bound trait's supertraits. */
export interface BoundProof {
  readonly boundIndex: number;
  /** Set when the bound's own trait reaches the required one only as a supertrait. */
  readonly supertrait?: { readonly sourceTraitIndex: number; readonly path: readonly number[] };
}

/**
 * Whether enclosing bound set `bounds` proves `parameter` implements the
 * trait: a bound on the trait itself, or on a trait that extends it, as
 * `T < Child` proves `T < Parent` (09-traits.md#r-trait.bound.supertraits),
 * with the supertrait path to it. Declarations, signatures, and the call
 * checker share this lookup.
 */
export function enclosingBoundProof(
  bounds: readonly HirGenericBound[],
  traitTypes: ReadonlyMap<string, HirTrait>,
  parameter: string,
  traitIndex: number,
  traitArguments: readonly ValueType[],
): BoundProof | undefined {
  const boundIndex = bounds.findIndex(
    (bound) =>
      bound.parameter === parameter &&
      bound.traitIndex === traitIndex &&
      bound.traitArguments.length === traitArguments.length &&
      bound.traitArguments.every((argument, index) => argument === traitArguments[index]),
  );
  if (boundIndex >= 0) return { boundIndex };
  for (const [index, bound] of bounds.entries()) {
    const trait = bound.parameter === parameter && traitTypes.get(bound.traitName);
    const path =
      trait &&
      findSupertraitPath(traitTypes, trait, bound.traitArguments, traitIndex, traitArguments);
    if (trait && path)
      return { boundIndex: index, supertrait: { sourceTraitIndex: trait.index, path: [...path] } };
  }
  return undefined;
}

export function findSupertraitPath(
  traitTypes: ReadonlyMap<string, HirTrait>,
  trait: HirTrait,
  traitArguments: readonly ValueType[],
  targetIndex: number,
  targetArguments: readonly ValueType[],
  seen: ReadonlySet<number> = new Set(),
): readonly number[] | undefined {
  if (seen.has(trait.index)) return undefined;
  const next = new Set([...seen, trait.index]);
  for (const [fieldIndex, supertrait] of trait.supertraits.entries()) {
    const substitutions = new Map(
      trait.genericParameters.map(
        (parameter, index) => [parameter, traitArguments[index]!] as const,
      ),
    );
    const arguments_ = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, substitutions),
    );
    if (
      supertrait.traitIndex === targetIndex &&
      arguments_.length === targetArguments.length &&
      arguments_.every((argument, index) => argument === targetArguments[index])
    )
      return [fieldIndex];
    const parent = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    );
    const rest =
      parent &&
      findSupertraitPath(traitTypes, parent, arguments_, targetIndex, targetArguments, next);
    if (rest) return [fieldIndex, ...rest];
  }
  return undefined;
}

export function resolveTraitPath(
  traitTypes: ReadonlyMap<string, HirTrait>,
  trait: HirTrait,
  traitArguments: readonly ValueType[],
  path: readonly number[],
): ResolvedTraitPath {
  let currentTrait = trait;
  let currentArguments = traitArguments;
  for (const fieldIndex of path) {
    const supertrait = currentTrait.supertraits[fieldIndex]!;
    const substitutions = new Map(
      currentTrait.genericParameters.map(
        (parameter, index) => [parameter, currentArguments[index]!] as const,
      ),
    );
    currentArguments = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, substitutions),
    );
    currentTrait = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    )!;
  }
  return { arguments: currentArguments, trait: currentTrait };
}

/**
 * The associated type bindings that the last supertrait edge of `path`
 * writes, as in `trait Summable < Add[Self, Out = Self]`, with the parent
 * trait's parameters substituted; `Self` stays `generic:Self`.
 */
export function supertraitPathBindings(
  traitTypes: ReadonlyMap<string, HirTrait>,
  trait: HirTrait,
  traitArguments: readonly ValueType[],
  path: readonly number[],
): readonly HirAssociatedBinding[] {
  let currentTrait = trait;
  let currentArguments = traitArguments;
  let bindings: readonly HirAssociatedBinding[] = [];
  for (const fieldIndex of path) {
    const supertrait = currentTrait.supertraits[fieldIndex]!;
    const substitutions = new Map(
      currentTrait.genericParameters.map(
        (parameter, index) => [parameter, currentArguments[index]!] as const,
      ),
    );
    bindings = (supertrait.associatedBindings ?? []).map((binding) => ({
      ...binding,
      type: substituteGenericType(binding.type, substitutions),
    }));
    currentArguments = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, substitutions),
    );
    currentTrait = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    )!;
  }
  return bindings;
}
