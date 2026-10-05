import type { HirTraitImplementation, ValueType } from "../hir.ts";
import { expandedAliasType, readonlyType } from "../types.ts";

// Implementation lookup tries the implementations whose target may match a
// type. A program with many derivations has many implementations, so the
// candidates for a nominal type are indexed by its declaration name.

/** The declaration name of a plain nominal type, such as `Map` of `Map[K,V]`. */
export const NOMINAL_HEAD = /^([A-Za-z_][A-Za-z0-9_]*)(?:\[.*\])?$/;

const IMPLEMENTATIONS_BY_HEAD = new WeakMap<
  readonly HirTraitImplementation[],
  { readonly length: number; readonly byHead: Map<string, readonly HirTraitImplementation[]> }
>();

/**
 * The implementations whose target may match `type`, in their order: for a
 * plain nominal type, only those of its declaration and those whose target
 * is not a plain nominal type, such as a generic parameter.
 */
export function implementationsFor(
  implementations: readonly HirTraitImplementation[],
  type: ValueType,
): readonly HirTraitImplementation[] {
  const head = NOMINAL_HEAD.exec(readonlyType(type))?.[1];
  if (head === undefined) return implementations;
  let index = IMPLEMENTATIONS_BY_HEAD.get(implementations);
  if (!index || index.length !== implementations.length) {
    index = { length: implementations.length, byHead: new Map() };
    IMPLEMENTATIONS_BY_HEAD.set(implementations, index);
  }
  // Transparent alias spellings share candidates (`usize` sees `u32` targets).
  const heads = new Set([head, expandedAliasType(head)]);
  const key = [...heads].sort().join("\0");
  let found = index.byHead.get(key);
  if (!found) {
    found = implementations.filter((implementation) => {
      const target = NOMINAL_HEAD.exec(implementation.targetType)?.[1];
      return target === undefined || heads.has(target) || heads.has(expandedAliasType(target));
    });
    index.byHead.set(key, found);
  }
  return found;
}
