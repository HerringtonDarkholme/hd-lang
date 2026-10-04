import type { HirData, HirEnum, ValueType } from "../hir.ts";
import {
  functionParts,
  functionType,
  mutableInner,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
} from "../types.ts";
import { conversionVariances, varianceConversion } from "./variance.ts";

interface TypeDeclarations {
  readonly data: ReadonlyMap<string, HirData>;
  readonly enums: ReadonlyMap<string, HirEnum>;
}

type LeastCommonType =
  | { readonly type: ValueType }
  | { readonly code: "no-common-type" | "no-least-common-type" };

type RepresentationExtremum =
  | { readonly type: ValueType }
  | { readonly code: "none" | "ambiguous" };

function representationConverts(
  source: ValueType,
  target: ValueType,
  declarations: TypeDeclarations,
): boolean {
  return (
    source === target ||
    mutableInner(source) === target ||
    varianceConversion(source, target, declarations) === true
  );
}

function selectExtremum(
  candidates: ReadonlySet<ValueType>,
  direction: "least" | "greatest",
  declarations: TypeDeclarations,
): RepresentationExtremum {
  const values = [...candidates];
  const extrema = values.filter((candidate) =>
    values.every((other) =>
      direction === "least"
        ? representationConverts(candidate, other, declarations)
        : representationConverts(other, candidate, declarations),
    ),
  );
  return extrema.length === 1
    ? { type: extrema[0]! }
    : { code: extrema.length === 0 ? "none" : "ambiguous" };
}

/**
 * A representation-preserving bound of structurally compatible types.
 * Nominal and function arguments are solved independently, avoiding a
 * Cartesian product across wide generic declarations.
 */
function representationExtremum(
  types: readonly ValueType[],
  direction: "least" | "greatest",
  declarations: TypeDeclarations,
): RepresentationExtremum {
  if (types.every((type) => type === types[0])) return { type: types[0]! };

  if (direction === "least") {
    const mutableViews = types
      .map(mutableInner)
      .filter((type): type is ValueType => type !== undefined);
    if (mutableViews.length > 0) {
      const fixed = mutableViews[0]!;
      return mutableViews.every((type) => type === fixed) &&
        types.every((type) => representationConverts(type, fixed, declarations))
        ? { type: fixed }
        : { code: "none" };
    }
  }

  const bounds = new Set(
    types.filter((candidate) =>
      types.every((type) =>
        direction === "least"
          ? representationConverts(type, candidate, declarations)
          : representationConverts(candidate, type, declarations),
      ),
    ),
  );
  let ambiguousStructure = false;

  const nominals = types.map(nominalGenericParts);
  const firstNominal = nominals[0];
  if (
    firstNominal &&
    nominals.every(
      (nominal) =>
        nominal?.name === firstNominal.name &&
        nominal.arguments.length === firstNominal.arguments.length,
    )
  ) {
    const variances = conversionVariances(
      firstNominal.name,
      firstNominal.arguments.length,
      declarations,
    );
    const arguments_: ValueType[] = [];
    let structural = variances.length === firstNominal.arguments.length;
    for (const [index, variance] of variances.entries()) {
      const inputs = nominals.map((nominal) => nominal!.arguments[index]!);
      if (!variance) {
        if (!inputs.every((type) => type === inputs[0])) structural = false;
        else arguments_.push(inputs[0]!);
        continue;
      }
      const argumentDirection =
        variance === "+" ? direction : direction === "least" ? "greatest" : "least";
      const result = representationExtremum(inputs, argumentDirection, declarations);
      if (!("type" in result)) {
        ambiguousStructure ||= result.code === "ambiguous";
        structural = false;
        continue;
      }
      arguments_.push(result.type);
    }
    if (structural) {
      const candidate = nominalGenericType(firstNominal.name, arguments_);
      if (
        types.every((type) =>
          direction === "least"
            ? representationConverts(type, candidate, declarations)
            : representationConverts(candidate, type, declarations),
        )
      )
        bounds.add(candidate);
    }
  }

  const callables = types.map(functionParts);
  const firstCallable = callables[0];
  if (
    firstCallable &&
    callables.every(
      (callable) =>
        callable?.suspending === firstCallable.suspending &&
        callable.variadic === firstCallable.variadic &&
        callable.parameters.length === firstCallable.parameters.length &&
        callable.requirements.join("+") === firstCallable.requirements.join("+"),
    )
  ) {
    const parameters: ValueType[] = [];
    let structural = true;
    for (const index of firstCallable.parameters.keys()) {
      const result = representationExtremum(
        callables.map((callable) => callable!.parameters[index]!),
        direction === "least" ? "greatest" : "least",
        declarations,
      );
      if (!("type" in result)) {
        ambiguousStructure ||= result.code === "ambiguous";
        structural = false;
        break;
      }
      parameters.push(result.type);
    }
    const result = structural
      ? representationExtremum(
          callables.map((callable) => callable!.result),
          direction,
          declarations,
        )
      : ({ code: "none" } as const);
    if ("type" in result) {
      const candidate = functionType(
        parameters,
        result.type,
        firstCallable.requirements,
        firstCallable.variadic,
        firstCallable.suspending,
      );
      if (
        types.every((type) =>
          direction === "least"
            ? representationConverts(type, candidate, declarations)
            : representationConverts(candidate, type, declarations),
        )
      )
        bounds.add(candidate);
    } else ambiguousStructure ||= result.code === "ambiguous";
  }

  const selected = selectExtremum(bounds, direction, declarations);
  return "code" in selected && selected.code === "none" && ambiguousStructure
    ? { code: "ambiguous" }
    : selected;
}

/** Widen only direct function values to the union of their requirement rows. */
function widenTopLevelRows(types: readonly ValueType[]): readonly ValueType[] {
  const values = types.filter((type) => type !== "never");
  const parts = values.map(functionParts);
  if (values.length === 0 || parts.some((part) => part === undefined)) return types;
  const requirements = [...new Set(parts.flatMap((part) => part!.requirements))].sort();
  let index = 0;
  return types.map((type) => {
    if (type === "never") return type;
    const part = parts[index++]!;
    return functionType(part.parameters, part.result, requirements, part.variadic, part.suspending);
  });
}

/** Whether one LCT input reaches a candidate by exactly the admitted rules. */
function convertsToLeastCandidate(
  source: ValueType,
  target: ValueType,
  declarations: TypeDeclarations,
  allowOptional = true,
): boolean {
  if (source === "never" || source === target) return true;
  // This is one outer permission weakening. Do not strip it and then apply
  // variance: types.lct.no-combine explicitly forbids that composition.
  if (mutableInner(source) === target) return true;
  if (varianceConversion(source, target, declarations) === true) return true;
  if (!allowOptional) return false;
  const payload = optionalInner(target);
  return payload !== undefined && convertsToLeastCandidate(source, payload, declarations, false);
}

/**
 * A wider reachability check used only to distinguish `no-least` from
 * `no-common`. It admits the permission-plus-variance composition that LCT
 * itself forbids, matching the specification's canonical ambiguity example.
 */
function hasOrdinaryCommonConversion(
  source: ValueType,
  target: ValueType,
  declarations: TypeDeclarations,
  allowOptional = true,
): boolean {
  if (convertsToLeastCandidate(source, target, declarations, allowOptional)) return true;
  const readonly = mutableInner(source);
  if (readonly !== undefined && varianceConversion(readonly, target, declarations) === true)
    return true;
  if (!allowOptional) return false;
  const payload = optionalInner(target);
  return payload !== undefined && hasOrdinaryCommonConversion(source, payload, declarations, false);
}

/**
 * The unique least common type, after direct function rows are widened.
 * Candidate construction is deliberately finite and naive; correctness does
 * not depend on a syntax-specific list of List or Map cases.
 */
export function leastCommonType(
  types: readonly ValueType[],
  declarations: TypeDeclarations,
): LeastCommonType {
  const values = widenTopLevelRows(types).filter((type) => type !== "never");
  if (values.length === 0) return { type: "never" };
  if (values.every((type) => type === values[0])) return { type: values[0]! };

  const representation = representationExtremum(values, "least", declarations);
  if ("type" in representation) return representation;
  if (representation.code === "ambiguous") return { code: "no-least-common-type" };

  if (values.some((type) => optionalInner(type) !== undefined)) {
    const payloads = values.map((type) => optionalInner(type) ?? type);
    const payload = leastCommonType(payloads, declarations);
    if (!("type" in payload)) return payload;
    const candidate = optionalType(payload.type);
    if (values.every((type) => convertsToLeastCandidate(type, candidate, declarations)))
      return { type: candidate };
  }

  const relaxed = representationExtremum(
    values.map((type) => mutableInner(type) ?? type),
    "least",
    declarations,
  );
  return "type" in relaxed &&
    values.every((type) => hasOrdinaryCommonConversion(type, relaxed.type, declarations))
    ? { code: "no-least-common-type" }
    : { code: "no-common-type" };
}

/** The LCT only when every non-never input is a direct function value. */
export function rowUnionType(
  types: readonly ValueType[],
  declarations: TypeDeclarations,
): ValueType | undefined {
  const values = types.filter((type) => type !== "never");
  if (values.length === 0 || values.some((type) => functionParts(type) === undefined))
    return undefined;
  const least = leastCommonType(types, declarations);
  return "type" in least ? least.type : undefined;
}
