import type { HirTrait } from "../hir.ts";

/**
 * The one-copy rule (09-traits.md#dynamic-safety). An associated type is safe
 * only when the value or requirement key binds it, so `bound` holds the names
 * that the use site binds (trait.dyn.binding.complete, req.key.binding.complete).
 */
export function traitIsDynamicallySafe(
  trait: HirTrait,
  traitTypes: ReadonlyMap<string, HirTrait>,
  bound: ReadonlySet<string> = new Set(),
  seen: ReadonlySet<number> = new Set(),
): boolean {
  if (seen.has(trait.index)) return true;
  if (
    trait.associatedTypes.some((associated) => !bound.has(associated.name)) ||
    trait.methods.some(
      (method) =>
        method.associated ||
        // A projection `Self::Item` is the bound type, not `Self`.
        method.parameters.some((parameter) => /generic:Self(?!::)/.test(parameter)) ||
        /generic:Self(?!::)/.test(method.result),
    )
  )
    return false;
  const next = new Set([...seen, trait.index]);
  return trait.supertraits.every((supertrait) => {
    const parent = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    );
    return !parent || traitIsDynamicallySafe(parent, traitTypes, bound, next);
  });
}
