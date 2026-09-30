import type { HirTrait, HirTraitDictionaryPlan, ValueType } from "../hir.ts";
import type { InherentMethod, Signature } from "./context.ts";
import { genericTypeName, mapKeyKind } from "./shared.ts";
import {
  functionParts,
  functionType,
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  storedSuspensionParts,
  tupleType,
} from "../types.ts";

// Assignability by permission weakening and readonly list variance, and the
// least common type built on it (04-type-system.md#assignability-and-coercion).

/**
 * The component pairs `[narrow, wide]` of a function-type variance conversion
 * from `actual` to `expected` (04-type-system.md#r-types.variance.function):
 * each parameter is contravariant, the result covariant, and the kind, arity,
 * vararg convention, and requirement row must match exactly.
 */
export function functionVariancePairs(
  actual: ValueType,
  expected: ValueType,
): readonly (readonly [ValueType, ValueType])[] | undefined {
  const from = functionParts(actual);
  const to = functionParts(expected);
  if (
    !from ||
    !to ||
    from.suspending !== to.suspending ||
    from.variadic !== to.variadic ||
    from.parameters.length !== to.parameters.length ||
    [...from.requirements].sort().join("+") !== [...to.requirements].sort().join("+")
  )
    return undefined;
  return [
    ...from.parameters.map((parameter, index) => [to.parameters[index]!, parameter] as const),
    [from.result, to.result] as const,
  ];
}

/**
 * True when the function value type `actual` fits the function type
 * `expected` only by row subsumption: `expected`'s row lists every key of
 * `actual`'s, and with that row `actual` is `expected` or weakens to it
 * (11-requirements-and-suspension.md#r-req.row.subsume). This converts a
 * function value itself, never a container of them, so it is not a variance
 * step (04-type-system.md#r-types.variance.function).
 */
export function isRowSubsumption(actual: ValueType, expected: ValueType): boolean {
  const from = functionParts(actual);
  const to = functionParts(expected);
  if (!from || !to) return false;
  const wide = new Set(to.requirements);
  // A row parameter the value lacks is still to be solved by the least-row
  // rule (11-requirements-and-suspension.md#least-row-solutions), not widened.
  if (
    from.requirements.length >= wide.size ||
    !from.requirements.every((requirement) => wide.has(requirement)) ||
    to.requirements.some(
      (requirement) => requirement.startsWith("row:") && !from.requirements.includes(requirement),
    )
  )
    return false;
  const widened = functionType(
    from.parameters,
    from.result,
    to.requirements,
    from.variadic,
    from.suspending,
  );
  return widened === expected || isPermissionWeakening(widened, expected);
}

export function isPermissionWeakening(actual: ValueType, expected: ValueType): boolean {
  const mutable = mutableInner(actual);
  if (mutable !== undefined)
    return mutable === expected || isPermissionWeakening(mutable, expected);
  const actualNominal = nominalGenericParts(actual);
  const expectedNominal = nominalGenericParts(expected);
  const actualCallable = functionParts(actual);
  const expectedCallable = functionParts(expected);
  if (
    actualCallable?.suspending &&
    expectedCallable &&
    !expectedCallable.suspending &&
    actualCallable.variadic === expectedCallable.variadic &&
    actualCallable.parameters.length === expectedCallable.parameters.length &&
    actualCallable.parameters.every(
      (parameter, index) => parameter === expectedCallable.parameters[index],
    ) &&
    actualCallable.requirements.length === expectedCallable.requirements.length &&
    actualCallable.requirements.every(
      (requirement, index) => requirement === expectedCallable.requirements[index],
    )
  ) {
    const stored = storedSuspensionParts(expectedCallable.result);
    return stored?.mutable === true && stored.result === actualCallable.result;
  }
  // Function types convert by their declared variance, which only changes
  // access permissions (07-functions.md#r-fn.type.variance-repr).
  const functionPairs = functionVariancePairs(actual, expected);
  if (functionPairs)
    return functionPairs.every(
      ([narrow, wide]) => narrow === wide || isPermissionWeakening(narrow, wide),
    );
  if (
    actualNominal?.name === "List" &&
    expectedNominal?.name === "List" &&
    actualNominal.arguments.length === 1 &&
    expectedNominal.arguments.length === 1
  ) {
    return (
      actualNominal.arguments[0] === expectedNominal.arguments[0] ||
      isPermissionWeakening(actualNominal.arguments[0]!, expectedNominal.arguments[0]!)
    );
  }
  // Readonly `Map[K, V]` is invariant in `K` and covariant in `V`
  // (04-type-system.md#variance).
  if (
    actualNominal?.name === "Map" &&
    expectedNominal?.name === "Map" &&
    actualNominal.arguments.length === 2 &&
    expectedNominal.arguments.length === 2 &&
    actualNominal.arguments[0] === expectedNominal.arguments[0]
  ) {
    return (
      actualNominal.arguments[1] === expectedNominal.arguments[1] ||
      isPermissionWeakening(actualNominal.arguments[1]!, expectedNominal.arguments[1]!)
    );
  }
  return false;
}

/**
 * The candidate types least-common-type inference may convert `type` to
 * (04-type-system.md#least-common-type): the type itself, its readonly view,
 * and, for a readonly list, the covariant variance step to a candidate of its
 * element. One candidate never combines permission weakening with a variance
 * step, so `mut List[mut U]` reaches `List[mut U]` but not `List[U]`.
 */
function leastTypeCandidates(type: ValueType): readonly ValueType[] {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return [type, mutable];
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1)
    return leastTypeCandidates(nominal.arguments[0]!).map((element) =>
      nominalGenericType("List", [element]),
    );
  return [type];
}

export type LeastCommonType =
  | { readonly type: ValueType }
  | { readonly code: "no-common-type" | "no-least-common-type" };

/**
 * The unique least common type of `types`, ignoring `never`; otherwise the
 * failure code: `no-least-common-type` when a common type exists under
 * ordinary assignability but the inference rules admit no unique least one.
 */
export function leastCommonType(types: readonly ValueType[]): LeastCommonType {
  const values = types.filter((type) => type !== "never");
  if (values.length === 0) return { type: types[0] ?? "never" };
  if (values.every((type) => type === values[0])) return { type: values[0]! };
  const assignable = (from: ValueType, to: ValueType): boolean =>
    from === to || isPermissionWeakening(from, to);
  let common = [...leastTypeCandidates(values[0]!)];
  for (const type of values.slice(1)) {
    const candidates = leastTypeCandidates(type);
    common = common.filter((candidate) => candidates.includes(candidate));
  }
  const least = common.filter((candidate) => common.every((other) => assignable(candidate, other)));
  if (least.length === 1) return { type: least[0]! };
  const readonlyView = values[0]!.replaceAll("mut:", "");
  return values.every((type) => assignable(type, readonlyView))
    ? { code: "no-least-common-type" }
    : { code: "no-common-type" };
}

/**
 * The least common type of function values in a list or map literal with no
 * expected type, after each value's row is widened to the union of their
 * rows (11-requirements-and-suspension.md#r-req.row.union.literal), or
 * undefined when a value is not a function type or the widened types still
 * have no least common type.
 */
export function rowUnionType(types: readonly ValueType[]): ValueType | undefined {
  const values = types.filter((type) => type !== "never");
  const parts = values.map((type) => functionParts(type));
  if (values.length === 0 || parts.some((part) => part === undefined)) return undefined;
  const union = [...new Set(parts.flatMap((part) => part!.requirements))].sort();
  const widened = parts.map((part) =>
    functionType(part!.parameters, part!.result, union, part!.variadic, part!.suspending),
  );
  const least = leastCommonType(widened);
  return "type" in least ? least.type : undefined;
}

/**
 * `std.iter.FromIterator[(K, V)]` for `Map[K, V]`, which std cannot write in
 * hd: a map built in generic code has no key equality for a type-parameter
 * key (spec/std/iter.md#r-std-iter.collect.map).
 */
export function mapCollectionPlan(
  traitIndex: number,
  type: ValueType,
  traitArguments: readonly ValueType[],
  inherentMethods: readonly InherentMethod[],
  signatures: ReadonlyMap<string, Signature>,
): HirTraitDictionaryPlan | undefined {
  const map = nominalGenericParts(type);
  if (map?.name !== "Map" || map.arguments.length !== 2) return undefined;
  if (traitArguments.length !== 1 || traitArguments[0] !== tupleType(map.arguments))
    return undefined;
  const keyKind = mapKeyKind(map.arguments[0]!);
  const next = inherentMethods.find(
    (method) =>
      method.name === "next" && nominalGenericParts(method.targetType)?.name === "Iterator",
  );
  const nextFunctionIndex = next && signatures.get(next.functionName)?.index;
  if (keyKind === undefined || nextFunctionIndex === undefined) return undefined;
  return {
    bounds: [],
    implementationIndex: -1,
    supertraits: [],
    builtin: { kind: "map-collection", traitIndex, targetType: type, keyKind, nextFunctionIndex },
  };
}

/**
 * The argument type a bounded parameter is inferred from: an argument passed
 * directly as a bounded `T` supplies its readonly view, while a `mut` inside a
 * type argument, as in `List[mut User]` for `List[T]`, stays part of `T`
 * (09-traits.md#erasure-to-inspectable).
 */
export function weakenBoundedGenericActual(
  formal: ValueType,
  actual: ValueType,
  bounded: ReadonlySet<string>,
  nested = false,
): ValueType {
  const generic = genericTypeName(formal);
  if (generic && bounded.has(generic)) return nested ? actual : readonlyType(actual);
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(readonlyType(actual));
  if (
    formalNominal &&
    actualNominal &&
    formalNominal.name === actualNominal.name &&
    formalNominal.arguments.length === actualNominal.arguments.length
  ) {
    return nominalGenericType(
      actualNominal.name,
      actualNominal.arguments.map((argument, index) =>
        weakenBoundedGenericActual(formalNominal.arguments[index]!, argument, bounded, true),
      ),
    );
  }
  return actual;
}

/**
 * A dynamic trait value type satisfies a bound on its own trait and on each
 * supertrait (09-traits.md#dynamic-trait-values): the dictionary forwards to
 * the value's own table, found along the supertrait path.
 */
export function forwardingPlan(
  traits: ReadonlyMap<string, HirTrait>,
  type: ValueType,
  traitIndex: number,
  traitArguments: readonly ValueType[],
): HirTraitDictionaryPlan | undefined {
  if (!type.startsWith("trait:")) return undefined;
  const byIndex = new Map([...traits.values()].map((trait) => [trait.index, trait] as const));
  const key = type.slice("trait:".length);
  const source = traits.get(nominalGenericParts(key)?.name ?? key);
  if (!source || traitArguments.length > 0) return undefined;
  const search = (
    trait: HirTrait,
    path: readonly { readonly traitIndex: number; readonly fieldIndex: number }[],
  ): typeof path | undefined => {
    if (trait.index === traitIndex) return path;
    for (const [fieldIndex, parent] of trait.supertraits.entries()) {
      const next = byIndex.get(parent.traitIndex);
      const found =
        next && parent.traitArguments.length === 0
          ? search(next, [...path, { traitIndex: trait.index, fieldIndex }])
          : undefined;
      if (found) return found;
    }
    return undefined;
  };
  const path = search(source, []);
  return path
    ? {
        bounds: [],
        implementationIndex: -1,
        supertraits: [],
        builtin: {
          kind: "forward",
          traitIndex,
          targetType: type,
          sourceTraitIndex: source.index,
          path,
        },
      }
    : undefined;
}
