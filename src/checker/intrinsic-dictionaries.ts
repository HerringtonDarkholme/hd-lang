import type {
  HirTrait,
  HirTraitDictionaryPlan,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import { readonlyType } from "../types.ts";
import { substituteGenericType } from "./shared.ts";

/**
 * The dictionary of an intrinsic implementation for a concrete type, such
 * as `Ord` for `i64` (09-traits.md#intrinsic-methods). The implementation
 * has no code, so the plan names each method's operation with its concrete
 * parameter and result types, and the emitter writes one wrapper per type
 * and method. `parent` finds each supertrait's dictionary for the type.
 */
export function intrinsicDictionaryPlan(
  implementation: HirTraitImplementation,
  trait: HirTrait,
  targetType: ValueType,
  substitutions: ReadonlyMap<string, ValueType>,
  parent: (traitIndex: number, traitArguments: readonly ValueType[]) => HirTraitDictionaryPlan,
): HirTraitDictionaryPlan {
  const type = readonlyType(targetType);
  const traitArguments = implementation.traitArguments.map((argument) =>
    substituteGenericType(argument, substitutions),
  );
  const traitSubstitutions = new Map<string, ValueType>([
    ...trait.genericParameters.map(
      (parameter, index) => [parameter, traitArguments[index]!] as const,
    ),
    ...trait.associatedTypes.map(
      (associated, index) =>
        [
          `Self::${associated.name}`,
          substituteGenericType(implementation.associatedTypes[index]!, substitutions),
        ] as const,
    ),
    ["Self", type],
  ]);
  const methods = trait.methods.map((method) => ({
    name: method.name,
    parameterTypes: method.parameters.map((parameter) =>
      substituteGenericType(parameter, traitSubstitutions),
    ),
    resultType: substituteGenericType(method.result, traitSubstitutions),
  }));
  const supertraits = trait.supertraits.map((supertrait) =>
    parent(
      supertrait.traitIndex,
      supertrait.traitArguments.map((argument) =>
        substituteGenericType(argument, traitSubstitutions),
      ),
    ),
  );
  return {
    bounds: [],
    implementationIndex: -1,
    supertraits,
    builtin: { kind: "intrinsic", traitIndex: trait.index, targetType: type, methods },
  };
}
