import {
  bindingParts,
  bindingType,
  contextKeys,
  contextType,
  functionParts,
  functionType,
  inputsInner,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  optionalType,
  restInner,
  rowArgumentKeys,
  rowArgumentType,
  tupleParts,
  tupleType,
} from "../types.ts";

/** Rename bound type names without rewriting nominal heads or binding labels. */
export function renameScopeType(type: string, names: ReadonlyMap<string, string>): string {
  if (names.size === 0) return type;
  const direct = names.get(type);
  if (direct) return direct;
  const visit = (inner: string): string => renameScopeType(inner, names);
  const binding = bindingParts(type);
  if (binding) return bindingType(binding, visit(binding.type));
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(visit(mutable));
  const inputs = inputsInner(type);
  if (inputs !== undefined) return `*${visit(inputs)}`;
  const rest = restInner(type);
  if (rest !== undefined) return `${visit(rest)}...`;
  const tuple = tupleParts(type);
  if (tuple) return tupleType(tuple.map(visit));
  const optional = optionalInner(type);
  if (optional !== undefined) return optionalType(visit(optional));
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map(visit),
      visit(callable.result),
      callable.requirements.map(visit),
      callable.variadic,
      callable.suspending,
    );
  const row = rowArgumentKeys(type);
  if (row) return rowArgumentType(row.map(visit));
  const context = contextKeys(type);
  if (context) return contextType(context.map(visit));
  for (const prefix of ["generic:", "row:", "trait:"])
    if (type.startsWith(prefix)) return `${prefix}${visit(type.slice(prefix.length))}`;
  const nominal = nominalGenericParts(type);
  if (nominal) return nominalGenericType(nominal.name, nominal.arguments.map(visit));
  const projection = type.indexOf("::");
  if (projection >= 0) return `${visit(type.slice(0, projection))}${type.slice(projection)}`;
  return type;
}
