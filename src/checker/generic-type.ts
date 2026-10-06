import type { ValueType } from "../hir.ts";
import { parsedType, type ParsedType } from "../types.ts";
import { rowParameterName } from "./requirement-rows.ts";

const CONTAINS_GENERIC_TYPE = new WeakMap<ParsedType, boolean>();

function inspect(type: ParsedType): boolean {
  if (type.generic !== undefined) return true;
  const inputs = type.inputs ?? type.rest;
  if (inputs) return containsGenericParsedType(inputs);
  if (type.binding) return containsGenericParsedType(type.binding.type);
  if (type.mutable) return containsGenericParsedType(type.mutable);
  if (type.tuple) return type.tuple.some(containsGenericParsedType);
  if (type.optional) return containsGenericParsedType(type.optional);
  if (type.result)
    return (
      containsGenericParsedType(type.result.ok) || containsGenericParsedType(type.result.error)
    );
  if (type.nominal) return type.nominal.arguments.some(containsGenericParsedType);
  const callable = type.callable;
  return Boolean(
    callable &&
    (callable.parameters.some(containsGenericParsedType) ||
      containsGenericParsedType(callable.result) ||
      callable.requirements.some(
        (requirement) =>
          !rowParameterName(requirement.text) && containsGenericParsedType(requirement),
      )),
  );
}

function containsGenericParsedType(type: ParsedType): boolean {
  const cached = CONTAINS_GENERIC_TYPE.get(type);
  if (cached !== undefined) return cached;
  const contains = inspect(type);
  CONTAINS_GENERIC_TYPE.set(type, contains);
  return contains;
}

export function containsGenericType(type: ValueType): boolean {
  return containsGenericParsedType(parsedType(type));
}
