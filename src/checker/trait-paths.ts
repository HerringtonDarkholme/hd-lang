import type { HirAssociatedBinding, HirTrait, ValueType } from "../hir.ts";
import type { ResolvedTraitPath } from "./context.ts";
import { substituteGenericType } from "./shared.ts";

// Paths from a trait to one of its transitive supertraits, as field indices
// of the supertrait lists (spec/lang/09-traits.md#supertraits).

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
