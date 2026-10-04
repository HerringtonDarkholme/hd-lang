import type { HirTrait, ValueType } from "../hir.ts";
import {
  bindingParts,
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  resultParts,
  splitTypeBindings,
  tupleParts,
  type TypeBinding,
  displayType,
} from "../types.ts";

// Associated type bindings by name (09-traits.md#binding-names): a binding
// may name an associated type that the trait declares or reaches through its
// supertraits, and a name that two declarations reachable that way share is
// ambiguous. A trait value type and a requirement key bind associated types
// as `Trait[Name=type]` (09-traits.md#bound-associated-types).

interface BindingProblem {
  readonly code: "unknown-associated-type" | "ambiguous-associated-type";
  readonly message: string;
}

function traitsByIndex(traitTypes: ReadonlyMap<string, HirTrait>): Map<number, HirTrait> {
  return new Map([...traitTypes.values()].map((trait) => [trait.index, trait] as const));
}

/** `trait` and its transitive supertraits, each once. */
function traitClosure(trait: HirTrait, traitTypes: ReadonlyMap<string, HirTrait>): HirTrait[] {
  const byIndex = traitsByIndex(traitTypes);
  const seen = new Map<number, HirTrait>();
  const visit = (current: HirTrait): void => {
    if (seen.has(current.index)) return;
    seen.set(current.index, current);
    for (const supertrait of current.supertraits) {
      const parent = byIndex.get(supertrait.traitIndex);
      if (parent) visit(parent);
    }
  };
  visit(trait);
  return [...seen.values()];
}

/** The traits among `trait` and its supertraits that declare the associated type `name`. */
function associatedDeclarations(
  trait: HirTrait,
  name: string,
  traitTypes: ReadonlyMap<string, HirTrait>,
): HirTrait[] {
  return traitClosure(trait, traitTypes).filter((candidate) =>
    candidate.associatedTypes.some((associated) => associated.name === name),
  );
}

/** Every associated type name `trait` declares or reaches. */
export function associatedNames(
  trait: HirTrait,
  traitTypes: ReadonlyMap<string, HirTrait>,
): Set<string> {
  return new Set(
    traitClosure(trait, traitTypes).flatMap((candidate) =>
      candidate.associatedTypes.map((associated) => associated.name),
    ),
  );
}

/** Why a binding of `name` on `trait` is invalid, if it is (trait.binding.name-reach). */
export function bindingNameProblem(
  trait: HirTrait,
  name: string,
  traitTypes: ReadonlyMap<string, HirTrait>,
): BindingProblem | undefined {
  const declarations = associatedDeclarations(trait, name, traitTypes);
  if (declarations.length === 0)
    return {
      code: "unknown-associated-type",
      message: `trait '${displayType(trait.name)}' declares or reaches no associated type '${name}'`,
    };
  if (declarations.length > 1)
    return {
      code: "ambiguous-associated-type",
      message: `'${name}' of '${displayType(trait.name)}' is ambiguous: ${declarations.map((candidate) => candidate.name).join(" and ")} each declare it`,
    };
  return undefined;
}

/** The trait and bindings of a trait value type or requirement key such as `Store[Item=User]`. */
export function traitKeyParts(key: string): {
  readonly name: string;
  readonly positional: readonly ValueType[];
  readonly bindings: readonly TypeBinding[];
} {
  const nominal = nominalGenericParts(key);
  if (!nominal) return { name: key, positional: [], bindings: [] };
  return { name: nominal.name, ...splitTypeBindings(nominal.arguments) };
}

/** The bindings of a (possibly `mut`) trait value type, by name. */
export function traitValueBindings(type: ValueType): ReadonlyMap<string, ValueType> {
  const readonly = mutableInner(type) ?? type;
  if (!readonly.startsWith("trait:")) return new Map();
  return new Map(
    traitKeyParts(readonly.slice("trait:".length)).bindings.map(
      (binding) => [binding.name, binding.type] as const,
    ),
  );
}

/**
 * The first problem with the bindings written in a resolved type: an unknown
 * or ambiguous name on a trait value type, or a binding on a type that is not
 * a trait (trait.binding.non-trait).
 */
export function writtenBindingProblem(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
): BindingProblem | undefined {
  const binding = bindingParts(type);
  if (binding) return writtenBindingProblem(binding.type, traitTypes);
  const inner = mutableInner(type) ?? optionalInner(type);
  if (inner !== undefined) return writtenBindingProblem(inner, traitTypes);
  const parts =
    tupleParts(type) ??
    (() => {
      const result = resultParts(type);
      if (result) return [result.ok, result.error];
      const callable = functionParts(type);
      return callable ? [...callable.parameters, callable.result] : undefined;
    })();
  if (parts) {
    for (const part of parts) {
      const problem = writtenBindingProblem(part, traitTypes);
      if (problem) return problem;
    }
    return undefined;
  }
  const isTrait = type.startsWith("trait:");
  const key = isTrait ? type.slice("trait:".length) : type;
  const nominal = nominalGenericParts(key);
  if (!nominal) return undefined;
  const { positional, bindings } = splitTypeBindings(nominal.arguments);
  const trait = isTrait ? traitTypes.get(nominal.name) : undefined;
  for (const written of bindings) {
    if (!trait)
      return {
        code: "unknown-associated-type",
        message: `'${nominal.name}' is not a trait, so it has no associated type '${written.name}'`,
      };
    const problem = bindingNameProblem(trait, written.name, traitTypes);
    if (problem) return problem;
  }
  for (const argument of [...positional, ...bindings.map((written) => written.type)]) {
    const problem = writtenBindingProblem(argument, traitTypes);
    if (problem) return problem;
  }
  return undefined;
}

/**
 * The first projection `P::Name` in a resolved type that two different
 * declarations reachable from `P`'s bounds declare. A binding does not make
 * it unambiguous (trait.assoc.ambiguous-type, trait.binding.ambiguous-projection).
 */
export function ambiguousProjection(
  type: ValueType,
  bounds: readonly { readonly parameter: string; readonly traitIndex: number }[],
  traitTypes: ReadonlyMap<string, HirTrait>,
): BindingProblem | undefined {
  const byIndex = traitsByIndex(traitTypes);
  for (const match of type.matchAll(/generic:([^?[\](),:]+)::([A-Za-z_][A-Za-z0-9_]*)/g)) {
    const [, parameter, name] = match;
    const declaring = new Map<number, HirTrait>();
    for (const bound of bounds) {
      const trait = bound.parameter === parameter ? byIndex.get(bound.traitIndex) : undefined;
      if (!trait) continue;
      for (const candidate of associatedDeclarations(trait, name!, traitTypes))
        declaring.set(candidate.index, candidate);
    }
    if (declaring.size > 1)
      return {
        code: "ambiguous-associated-type",
        message: `'${parameter}::${name}' is ambiguous: ${[...declaring.values()].map((trait) => trait.name).join(" and ")} each declare '${name}'`,
      };
  }
  return undefined;
}
