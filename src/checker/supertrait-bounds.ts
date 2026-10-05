// An implementation proves each supertrait under its own bounds
// (09-traits.md#r-trait.super.impl-bounds): the implementation that meets a
// supertrait for the target may have bounds, and the implementing one's own
// bounds, or implementations of concrete types, must meet them.

import type { ImplDecl } from "../ast.ts";
import type { HirTrait, ValueType } from "../hir.ts";
import { mutableInner, nominalGenericParts } from "../types.ts";
import { numericType } from "../numeric.ts";
import type { ImplementationPreparation, ProgramCheckContext } from "./program-context.ts";
import {
  traitImpliesValueCategory,
  typeSatisfiesValueCategory,
  type ValueCategory,
} from "./value-categories.ts";
import {
  genericTypeName,
  matchTraitImplementation,
  MAX_BOUND_DEPTH,
  resolveGenericType,
  substituteGenericType,
} from "./shared.ts";

interface Requirement {
  readonly trait: HirTrait;
  readonly arguments: readonly ValueType[];
}

/**
 * Whether the primitive number `type` meets the argument-free `trait` without
 * a written implementation: `Eq`, `PartialOrd`, and `Display` for every
 * number type, and `Ord` for integers (05-expressions.md#equality, #ordering).
 */
export function builtInSupertraitHolds(traitName: string, targetType: ValueType): boolean {
  const numeric = numericType(targetType);
  if (!numeric) return false;
  if (traitName === "Ord") return numeric.family !== "float";
  return traitName === "Eq" || traitName === "PartialOrd" || traitName === "Display";
}

/** The trait a written bound such as `Holder[T]` names, with `matched` substituted. */
function writtenRequirement(
  written: string,
  parameters: ReadonlySet<string>,
  matched: ReadonlyMap<string, ValueType>,
  context: ProgramCheckContext,
): Requirement | undefined {
  const resolved = substituteGenericType(
    resolveGenericType(mutableInner(written) ?? written, parameters),
    matched,
  );
  const parts = nominalGenericParts(resolved);
  const trait = context.traitTypes.get(parts?.name ?? resolved);
  return trait && { trait, arguments: parts?.arguments ?? [] };
}

/** Whether `trait` with `arguments`, or one of its supertraits, is `wanted`. */
function implies(
  trait: HirTrait,
  arguments_: readonly ValueType[],
  wanted: Requirement,
  context: ProgramCheckContext,
  depth: number,
): boolean {
  if (
    trait.index === wanted.trait.index &&
    arguments_.length === wanted.arguments.length &&
    arguments_.every((argument, index) => argument === wanted.arguments[index])
  )
    return true;
  if (depth > MAX_BOUND_DEPTH) return false;
  const parameters = new Map(
    trait.genericParameters.map((name, index) => [name, arguments_[index]!] as const),
  );
  return trait.supertraits.some((supertrait) => {
    const parent = [...context.traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    );
    return (
      parent !== undefined &&
      implies(
        parent,
        supertrait.traitArguments.map((argument) => substituteGenericType(argument, parameters)),
        wanted,
        context,
        depth + 1,
      )
    );
  });
}

/** Whether `type` meets `wanted` under the bounds of `own`. */
function meets(
  type: ValueType,
  wanted: Requirement,
  own: ImplDecl,
  context: ProgramCheckContext,
  depth: number,
): boolean {
  if (depth > MAX_BOUND_DEPTH) return false;
  const parameter = genericTypeName(type);
  if (parameter !== undefined) {
    const parameters = new Set(own.genericParameters);
    return own.genericBounds.some(
      (bound) =>
        bound.parameter === parameter &&
        bound.traits.some((written) => {
          const found = writtenRequirement(written, parameters, new Map(), context);
          return found !== undefined && implies(found.trait, found.arguments, wanted, context, 0);
        }),
    );
  }
  if (wanted.arguments.length === 0 && builtInSupertraitHolds(wanted.trait.name, type)) return true;
  return context.implementationPreparations.some((candidate) => {
    const matched = matchTraitImplementation(candidate, wanted.trait.index, type, wanted.arguments);
    return matched !== undefined && boundsHold(candidate, matched, own, context, depth + 1);
  });
}

/**
 * Whether the bounds of `candidate`, an implementation matched with
 * `matched`, hold under the bounds of `own`.
 */
export function boundsHold(
  candidate: ImplementationPreparation,
  matched: ReadonlyMap<string, ValueType>,
  own: ImplDecl,
  context: ProgramCheckContext,
  depth = 0,
): boolean {
  const parameters = new Set(candidate.declaration.genericParameters);
  return candidate.declaration.genericBounds.every((bound) => {
    const actual = matched.get(bound.parameter);
    if (actual === undefined) return true;
    return bound.traits.every((written) => {
      if (written === "AnyVal" || written === "AnyRef")
        return typeSatisfiesValueCategory(actual, written, {
          dataTypes: context.dataTypes,
          enumTypes: context.enumTypes,
          [written === "AnyRef" ? "referenceParameters" : "valueParameters"]:
            implementationCategoryParameters(own, context.traitTypes, written),
        });
      const wanted = writtenRequirement(written, parameters, matched, context);
      // `Any` holds for every type; an unknown name is reported where the
      // bound resolves.
      return wanted === undefined || meets(actual, wanted, own, context, depth);
    });
  });
}

/** Generic implementation parameters whose written bounds prove one category. */
export function implementationCategoryParameters(
  implementation: ImplDecl,
  traitTypes: ReadonlyMap<string, HirTrait>,
  category: ValueCategory,
): Set<string> {
  return new Set(
    implementation.genericParameters.filter((parameter) =>
      implementation.genericBounds.some(
        (bound) =>
          bound.parameter === parameter &&
          bound.traits.some((written) => {
            const key = mutableInner(written) ?? written;
            const name = nominalGenericParts(key)?.name ?? key;
            if (name === category) return true;
            const trait = traitTypes.get(name);
            return trait !== undefined && traitImpliesValueCategory(trait, traitTypes, category);
          }),
      ),
    ),
  );
}
