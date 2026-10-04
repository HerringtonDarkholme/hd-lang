import type { HirGenericBound, HirTrait, ValueType } from "../hir.ts";
import {
  bindingParts,
  contextKeys,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  nonListRestElement,
  optionalInner,
  readonlyType,
  restInner,
  resultParts,
  rowArgumentKeys,
  tupleParts,
  typeSourceText,
} from "../types.ts";
import {
  ambiguousProjection,
  traitValueBindings,
  writtenBindingProblem,
} from "./associated-bindings.ts";
import { traitIsDynamicallySafe } from "./dynamic-safety.ts";

interface TypeProblem {
  readonly code: string;
  readonly message: string;
}

export function traitTypeName(type: ValueType): string | undefined {
  const readonly = readonlyType(type);
  if (!readonly.startsWith("trait:") || readonly.endsWith("?")) return undefined;
  const key = readonly.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
}

/** The first trait value in `type` that cannot be used for dynamic dispatch. */
export function dynamicTraitProblemInType(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
): TypeProblem | undefined {
  const visit = (current: ValueType): TypeProblem | undefined => {
    const binding = bindingParts(current);
    if (binding) return visit(binding.type);
    const wrapper = restInner(current) ?? inputsInner(current) ?? mutableInner(current);
    if (wrapper !== undefined) return visit(wrapper);
    const optional = optionalInner(current);
    if (optional !== undefined) return visit(optional);

    const dynamicTraitName = traitTypeName(current);
    const dynamicTrait = dynamicTraitName && traitTypes.get(dynamicTraitName);
    if (
      dynamicTrait &&
      !traitIsDynamicallySafe(dynamicTrait, traitTypes, new Set(traitValueBindings(current).keys()))
    )
      return {
        code: "trait-not-dynamically-safe",
        message: `trait '${dynamicTrait.name}' cannot be used as a dynamic value`,
      };

    const tuple = tupleParts(current);
    if (tuple) {
      for (const element of tuple) {
        const problem = visit(element);
        if (problem) return problem;
      }
      return undefined;
    }
    const result = resultParts(current);
    if (result) return visit(result.ok) ?? visit(result.error);
    const callable = functionParts(current);
    if (callable) {
      for (const key of callable.requirements) {
        for (const argument of nominalGenericParts(key)?.arguments ?? []) {
          const problem = visit(argument);
          if (problem) return problem;
        }
      }
      for (const parameter of callable.parameters) {
        const problem = visit(parameter);
        if (problem) return problem;
      }
      return visit(callable.result);
    }
    const requirementKeys = contextKeys(current) ?? rowArgumentKeys(current);
    if (requirementKeys) {
      for (const key of requirementKeys) {
        for (const argument of nominalGenericParts(key)?.arguments ?? []) {
          const problem = visit(argument);
          if (problem) return problem;
        }
      }
      return undefined;
    }
    const nominal = nominalGenericParts(current);
    if (nominal) {
      for (const argument of nominal.arguments) {
        const problem = visit(argument);
        if (problem) return problem;
      }
    }
    return undefined;
  };
  return visit(type);
}

/** A rest element that is not a `List[T]` (04-type-system.md#r-types.tuple.rest.list). */
export function restElementProblem(type: ValueType): TypeProblem | undefined {
  const rest = nonListRestElement(type);
  return rest === undefined
    ? undefined
    : {
        code: "type-mismatch",
        message: `a rest element must be a List[T], as in 'List[${typeSourceText(rest)}]...', not '${typeSourceText(rest)}...'`,
      };
}

/** Structural problems shared by declaration and local written types. */
export function writtenTypeProblem(
  type: ValueType,
  bounds: readonly HirGenericBound[],
  traitTypes: ReadonlyMap<string, HirTrait>,
): TypeProblem | undefined {
  return (
    restElementProblem(type) ??
    writtenBindingProblem(type, traitTypes) ??
    ambiguousProjection(type, bounds, traitTypes)
  );
}
