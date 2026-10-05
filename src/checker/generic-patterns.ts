import type { ValueType } from "../hir.ts";
import {
  bindingParts,
  expandedAliasType,
  functionInputsTuple,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  restInner,
  rowArgumentType,
  tupleParts,
  type FunctionParts,
} from "../types.ts";
import { NOMINAL_HEAD } from "./implementation-index.ts";
import { rowParameterName, sameRequirements } from "./requirement-rows.ts";

const genericTypeName = (type: ValueType): string | undefined =>
  /^generic:([^?[\](),]+)$/.exec(type)?.[1];

const tupleInputs = (callable: FunctionParts): ValueType | undefined =>
  callable.parameters.length === 1 ? inputsInner(callable.parameters[0]!) : undefined;

/** Structurally match a generic type pattern and retain each binder identity. */
export function matchGenericTypePattern(
  pattern: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
): boolean {
  const patternHead = NOMINAL_HEAD.exec(pattern)?.[1];
  if (patternHead !== undefined) {
    const actualHead = NOMINAL_HEAD.exec(actual)?.[1];
    if (
      actualHead !== undefined &&
      expandedAliasType(actualHead) !== expandedAliasType(patternHead)
    )
      return false;
  }
  const patternBinding = bindingParts(pattern);
  const actualBinding = bindingParts(actual);
  if (patternBinding || actualBinding)
    return Boolean(
      patternBinding &&
      actualBinding &&
      patternBinding.name === actualBinding.name &&
      matchGenericTypePattern(patternBinding.type, actualBinding.type, substitutions),
    );
  const patternRest = restInner(pattern);
  const actualRest = restInner(actual);
  if (patternRest !== undefined || actualRest !== undefined)
    return Boolean(
      patternRest !== undefined &&
      actualRest !== undefined &&
      matchGenericTypePattern(patternRest, actualRest, substitutions),
    );
  const generic = genericTypeName(pattern);
  if (generic) {
    const existing = substitutions.get(generic);
    if (existing) return expandedAliasType(existing) === expandedAliasType(actual);
    substitutions.set(generic, actual);
    return true;
  }
  if (
    (pattern === actual || expandedAliasType(pattern) === expandedAliasType(actual)) &&
    !pattern.includes("generic:")
  )
    return true;
  const patternMutable = mutableInner(pattern);
  const actualMutable = mutableInner(actual);
  if (patternMutable !== undefined || actualMutable !== undefined)
    return (
      patternMutable !== undefined &&
      actualMutable !== undefined &&
      matchGenericTypePattern(patternMutable, actualMutable, substitutions)
    );
  const patternOptional = optionalInner(pattern);
  const actualOptional = optionalInner(actual);
  if (patternOptional !== undefined || actualOptional !== undefined)
    return (
      patternOptional !== undefined &&
      actualOptional !== undefined &&
      matchGenericTypePattern(patternOptional, actualOptional, substitutions)
    );
  const patternTuple = tupleParts(pattern);
  const actualTuple = tupleParts(actual);
  if (patternTuple !== undefined || actualTuple !== undefined)
    return Boolean(
      patternTuple &&
      actualTuple &&
      patternTuple.length === actualTuple.length &&
      patternTuple.every((element, index) =>
        matchGenericTypePattern(element, actualTuple[index]!, substitutions),
      ),
    );
  const patternCallable = functionParts(pattern);
  const actualCallable = functionParts(actual);
  if (patternCallable || actualCallable) {
    if (
      !patternCallable ||
      !actualCallable ||
      patternCallable.suspending !== actualCallable.suspending ||
      !matchGenericTypePattern(patternCallable.result, actualCallable.result, substitutions)
    )
      return false;
    const rows = patternCallable.requirements.map(rowParameterName);
    if (rows.length === 1 && rows[0] !== undefined) {
      const row = rowArgumentType(actualCallable.requirements);
      const existing = substitutions.get(rows[0]);
      if (existing !== undefined && existing !== row) return false;
      substitutions.set(rows[0], row);
    } else if (!sameRequirements(patternCallable.requirements, actualCallable.requirements))
      return false;
    const inputs = tupleInputs(patternCallable);
    if (inputs !== undefined)
      return matchGenericTypePattern(inputs, functionInputsTuple(actualCallable), substitutions);
    return matchGenericTypePattern(
      functionInputsTuple(patternCallable),
      functionInputsTuple(actualCallable),
      substitutions,
    );
  }
  const patternNominal = nominalGenericParts(pattern);
  const actualNominal = nominalGenericParts(actual);
  return Boolean(
    patternNominal &&
    actualNominal &&
    patternNominal.name === actualNominal.name &&
    patternNominal.arguments.length === actualNominal.arguments.length &&
    patternNominal.arguments.every((argument, index) =>
      matchGenericTypePattern(argument, actualNominal.arguments[index]!, substitutions),
    ),
  );
}
