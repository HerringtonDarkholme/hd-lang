import type { ValueType } from "../hir.ts";
import {
  bindingParts,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  restInner,
  resultParts,
  tupleParts,
} from "../types.ts";
import { rowParameterName } from "./requirement-rows.ts";

const CONTAINS_GENERIC_TYPE = new Map<ValueType, boolean>();
const GENERIC_TYPE = /^generic:([^?[\](),]+)$/;

function inspect(type: ValueType): boolean {
  if (GENERIC_TYPE.test(type)) return true;
  const inputs = inputsInner(type) ?? restInner(type);
  if (inputs !== undefined) return containsGenericType(inputs);
  const binding = bindingParts(type);
  if (binding) return containsGenericType(binding.type);
  const mutable = mutableInner(type);
  if (mutable !== undefined) return containsGenericType(mutable);
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tuple.some(containsGenericType);
  const optional = optionalInner(type);
  if (optional !== undefined) return containsGenericType(optional);
  const result = resultParts(type);
  if (result) return containsGenericType(result.ok) || containsGenericType(result.error);
  const nominal = nominalGenericParts(type);
  if (nominal) return nominal.arguments.some(containsGenericType);
  const callable = functionParts(type);
  return Boolean(
    callable &&
    (callable.parameters.some(containsGenericType) ||
      containsGenericType(callable.result) ||
      callable.requirements.some(
        (requirement) => !rowParameterName(requirement) && containsGenericType(requirement),
      )),
  );
}

export function containsGenericType(type: ValueType): boolean {
  const cached = CONTAINS_GENERIC_TYPE.get(type);
  if (cached !== undefined) return cached;
  const contains = inspect(type);
  CONTAINS_GENERIC_TYPE.set(type, contains);
  return contains;
}
