import type { HirTraitDictionaryPlan, ValueType } from "../hir.ts";
import { genericTypeName } from "./shared.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  storedSuspensionParts,
  tupleType,
} from "../types.ts";

// Assignability by permission weakening and readonly list variance, and the
// least common type built on it (04-type-system.md#assignability-and-coercion).

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
 * `List[T]` is `Iterable[T]` and `Map[K, V]` is `Iterable[(K, V)]`
 * (06-control-flow.md#for-loops); their compiler-supplied dictionary returns a
 * cursor.
 */
export function collectionIterablePlan(
  traitIndex: number,
  type: ValueType,
  traitArguments: readonly ValueType[],
): HirTraitDictionaryPlan | undefined {
  const collection = nominalGenericParts(type);
  const element =
    collection?.name === "List" && collection.arguments.length === 1
      ? collection.arguments[0]
      : collection?.name === "Map" && collection.arguments.length === 2
        ? tupleType(collection.arguments)
        : undefined;
  if (traitArguments.length !== 1 || element === undefined || element !== traitArguments[0])
    return undefined;
  return {
    bounds: [],
    implementationIndex: -1,
    supertraits: [],
    builtin: { kind: "iterable", traitIndex, targetType: type },
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
