import type { HirGenericBound, HirTrait, HirTraitImplementation, ValueType } from "../hir.ts";
import { readonlyType } from "../types.ts";
import { implementationsFor } from "./implementation-index.ts";
import type { Signature } from "./context.ts";
import {
  containsGenericParameter,
  containsGenericType,
  genericTypeName,
  inferGenericType,
  matchImplementationTarget,
  matchTraitImplementation,
  substituteGenericType,
} from "./shared.ts";

type BuiltinImplementation = (
  traitIndex: number,
  target: ValueType,
  traitArguments: readonly ValueType[],
) => boolean;

/**
 * Solves parameters named by trait arguments from each known bounded
 * receiver's unique trait instantiation, repeating to a fixed point.
 */
export function inferTypesThroughBounds(
  signature: Signature,
  substitutions: Map<string, ValueType>,
  callerBounds: readonly HirGenericBound[],
  implementations: readonly HirTraitImplementation[],
  traitTypes: ReadonlyMap<string, HirTrait>,
  builtinImplementation: BuiltinImplementation,
): HirGenericBound | undefined {
  const mentioningBounds = (parameter: string): readonly HirGenericBound[] =>
    signature.genericBounds.filter((bound) =>
      bound.traitArguments.some((argument) => containsGenericParameter(argument, parameter)),
    );
  const inheritedInstantiations = (
    trait: HirTrait,
    arguments_: readonly ValueType[],
    targetIndex: number,
    seen: ReadonlySet<number> = new Set(),
  ): readonly (readonly ValueType[])[] => {
    if (seen.has(trait.index)) return [];
    if (trait.index === targetIndex) return [arguments_];
    const next = new Set([...seen, trait.index]);
    const parameters = new Map(
      trait.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
    );
    return trait.supertraits.flatMap((supertrait) => {
      const parent = [...traitTypes.values()].find(
        (candidate) => candidate.index === supertrait.traitIndex,
      );
      return parent
        ? inheritedInstantiations(
            parent,
            supertrait.traitArguments.map((argument) =>
              substituteGenericType(argument, parameters),
            ),
            targetIndex,
            next,
          )
        : [];
    });
  };
  const implementationAvailable = (
    implementation: HirTraitImplementation,
    implementationSubstitutions: ReadonlyMap<string, ValueType>,
    seen: ReadonlySet<string> = new Set(),
  ): boolean => {
    const key = `${implementation.index}:${[...implementationSubstitutions].join("=")}`;
    if (seen.has(key)) return false;
    const next = new Set([...seen, key]);
    return implementation.genericBounds.every((bound) => {
      const actual = implementationSubstitutions.get(bound.parameter);
      if (!actual) return false;
      const target = readonlyType(actual);
      const traitArguments = bound.traitArguments.map((argument) =>
        substituteGenericType(argument, implementationSubstitutions),
      );
      if (traitArguments.some(containsGenericType)) return false;
      if (builtinImplementation(bound.traitIndex, target, traitArguments)) return true;
      return implementationsFor(implementations, target).some((provider) => {
        const providerSubstitutions = matchTraitImplementation(
          provider,
          bound.traitIndex,
          target,
          traitArguments,
        );
        return (
          providerSubstitutions !== undefined &&
          implementationAvailable(provider, providerSubstitutions, next)
        );
      });
    });
  };
  const instantiationsFor = (bound: HirGenericBound): readonly (readonly ValueType[])[] => {
    const target = readonlyType(substitutions.get(bound.parameter)!);
    const found = new Map<string, readonly ValueType[]>();
    const forwarded = genericTypeName(target);
    if (forwarded) {
      for (const callerBound of callerBounds) {
        if (callerBound.parameter !== forwarded) continue;
        const trait = traitTypes.get(callerBound.traitName);
        if (!trait) continue;
        for (const arguments_ of inheritedInstantiations(
          trait,
          callerBound.traitArguments,
          bound.traitIndex,
        ))
          found.set(JSON.stringify(arguments_), arguments_);
      }
      return [...found.values()];
    }
    for (const implementation of implementationsFor(implementations, target)) {
      if (implementation.traitIndex !== bound.traitIndex) continue;
      const targetSubstitutions = new Map<string, ValueType>();
      if (!matchImplementationTarget(implementation, target, targetSubstitutions)) continue;
      const actualArguments = implementation.traitArguments.map((argument) =>
        substituteGenericType(argument, targetSubstitutions),
      );
      if (actualArguments.some(containsGenericType)) continue;
      const complete = matchTraitImplementation(
        implementation,
        bound.traitIndex,
        target,
        actualArguments,
      );
      if (complete && implementationAvailable(implementation, complete))
        found.set(JSON.stringify(actualArguments), actualArguments);
    }
    return [...found.values()].filter((actualArguments) => {
      const trial = new Map(substitutions);
      return !bound.traitArguments.some((argument, index) =>
        inferGenericType(argument, actualArguments[index]!, trial),
      );
    });
  };
  const candidatesFor = (bound: HirGenericBound, parameter: string): Set<ValueType> => {
    const candidates = new Set<ValueType>();
    for (const actualArguments of instantiationsFor(bound)) {
      const trial = new Map(substitutions);
      const conflict = bound.traitArguments.some((argument, index) =>
        inferGenericType(argument, actualArguments[index]!, trial),
      );
      const candidate = conflict ? undefined : trial.get(parameter);
      if (candidate !== undefined) candidates.add(candidate);
    }
    return candidates;
  };

  let changed: boolean;
  do {
    changed = false;
    for (const parameter of signature.genericParameters) {
      if (substitutions.has(parameter)) continue;
      const bounds = mentioningBounds(parameter);
      if (bounds.length === 0 || bounds.some((bound) => !substitutions.has(bound.parameter)))
        continue;
      const instantiations = bounds.map(instantiationsFor);
      if (!instantiations.every((found) => found.length === 1)) continue;
      const allowed = bounds.map((bound) => candidatesFor(bound, parameter));
      if (allowed.some((values) => values.size !== 1)) continue;
      const candidates = new Set(allowed.map((values) => [...values][0]!));
      if (candidates.size !== 1) continue;
      substitutions.set(parameter, [...candidates][0]!);
      changed = true;
    }
  } while (changed);

  for (const parameter of signature.genericParameters) {
    if (substitutions.has(parameter)) continue;
    const bounds = mentioningBounds(parameter);
    if (bounds.length === 0 || bounds.some((bound) => !substitutions.has(bound.parameter)))
      continue;
    const missing = bounds.find((bound) => instantiationsFor(bound).length === 0);
    if (missing) return missing;
  }
  return undefined;
}
