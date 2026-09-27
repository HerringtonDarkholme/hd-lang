import type { VarianceMarker } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type { HirData, HirEnum, ValueType } from "../hir.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  tupleParts,
} from "../types.ts";
import { functionVariancePairs, isPermissionWeakening } from "./assignability.ts";
import { genericTypeName } from "./shared.ts";

// Declared variance (04-type-system.md#variance). A `+T` parameter may occur
// only in positive positions of the type's readonly surface and a `-T`
// parameter only in negative ones; `mut`, an embedded field, and an
// invariant argument make an occurrence invariant.

type Polarity = 1 | -1 | 0;

type Declarations = {
  readonly data: ReadonlyMap<string, HirData>;
  readonly enums: ReadonlyMap<string, HirEnum>;
};

function variancesOf(name: string, declarations: Declarations): readonly VarianceMarker[] {
  return declarations.data.get(name)?.variances ?? declarations.enums.get(name)?.variances ?? [];
}

function occurrences(
  type: ValueType,
  polarity: Polarity,
  declarations: Declarations,
  found: [string, Polarity][],
): void {
  const flip = (value: Polarity): Polarity => (value === 0 ? 0 : value === 1 ? -1 : 1);
  const generic = genericTypeName(type);
  if (generic) {
    found.push([generic, polarity]);
    return;
  }
  const mutable = mutableInner(type);
  if (mutable !== undefined) return occurrences(mutable, 0, declarations, found);
  const optional = optionalInner(type);
  if (optional !== undefined) return occurrences(optional, polarity, declarations, found);
  const tuple = tupleParts(type);
  if (tuple) {
    for (const element of tuple) occurrences(element, polarity, declarations, found);
    return;
  }
  const callable = functionParts(type);
  if (callable) {
    for (const parameter of callable.parameters)
      occurrences(parameter, flip(polarity), declarations, found);
    occurrences(callable.result, polarity, declarations, found);
    return;
  }
  if (type.startsWith("trait:")) {
    for (const argument of nominalGenericParts(type.slice("trait:".length))?.arguments ?? [])
      occurrences(argument, 0, declarations, found);
    return;
  }
  const nominal = nominalGenericParts(type);
  if (!nominal) return;
  const markers: readonly VarianceMarker[] =
    nominal.name === "List" || nominal.name === "Result"
      ? nominal.arguments.map(() => "+")
      : nominal.name === "Map"
        ? [undefined, "+"]
        : variancesOf(nominal.name, declarations);
  nominal.arguments.forEach((argument, index) => {
    const marker = markers[index];
    occurrences(
      argument,
      marker === "+" ? polarity : marker === "-" ? flip(polarity) : 0,
      declarations,
      found,
    );
  });
}

/** `invalid-variance` for every field whose type uses a parameter against its marker. */
export function varianceDiagnostics(declarations: Declarations): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const check = (
    parameters: readonly string[],
    variances: readonly VarianceMarker[] | undefined,
    fields: readonly {
      readonly type: ValueType;
      readonly embedded?: boolean;
      readonly span: Diagnostic["span"];
    }[],
  ): void => {
    if (!variances?.some(Boolean)) return;
    for (const field of fields) {
      const found: [string, Polarity][] = [];
      occurrences(field.type, field.embedded ? 0 : 1, declarations, found);
      const wrong = found.find(([name, polarity]) => {
        const marker = variances[parameters.indexOf(name)];
        return (marker === "+" && polarity !== 1) || (marker === "-" && polarity !== -1);
      });
      if (wrong) {
        const marker = variances[parameters.indexOf(wrong[0])];
        diagnostics.push({
          code: "invalid-variance",
          message: `'${marker}${wrong[0]}' occurs in a${wrong[1] === 0 ? "n invariant" : wrong[1] === 1 ? " positive" : " negative"} position`,
          span: field.span,
        });
      }
    }
  };
  for (const data of declarations.data.values())
    check(data.genericParameters, data.variances, data.fields);
  for (const declaration of declarations.enums.values())
    check(declaration.genericParameters, declaration.variances, [
      ...declaration.sharedFields,
      ...declaration.variants.flatMap((variant) => variant.fields),
    ]);
  return diagnostics;
}

/**
 * Whether a readonly view `D[S]` converts to `D[T]` by declared variance
 * (04-type-system.md#variance): each covariant argument converts `S -> T` and
 * each contravariant one `T -> S` by a representation-preserving step.
 * "representation-change" reports a conversion that would wrap a value.
 */
export function varianceConversion(
  source: ValueType,
  target: ValueType,
  declarations: Declarations,
): boolean | "representation-change" {
  const functionPairs = functionVariancePairs(readonlyType(source), target);
  if (functionPairs) {
    for (const [narrow, wide] of functionPairs) {
      if (narrow === wide || isPermissionWeakening(narrow, wide)) continue;
      return wide.startsWith("trait:") || optionalInner(wide) === narrow
        ? "representation-change"
        : false;
    }
    return true;
  }
  const from = nominalGenericParts(readonlyType(source));
  const to = mutableInner(target) === undefined ? nominalGenericParts(target) : undefined;
  if (!from || !to || from.name !== to.name || from.arguments.length !== to.arguments.length)
    return false;
  const markers = variancesOf(from.name, declarations);
  if (!markers.some(Boolean)) return false;
  for (const [index, argument] of from.arguments.entries()) {
    const wanted = to.arguments[index]!;
    if (argument === wanted) continue;
    const marker = markers[index];
    if (!marker) return false;
    const [narrow, wide] = marker === "+" ? [argument, wanted] : [wanted, argument];
    if (isPermissionWeakening(narrow, wide)) continue;
    return wide.startsWith("trait:") || optionalInner(wide) === narrow
      ? "representation-change"
      : false;
  }
  return true;
}
