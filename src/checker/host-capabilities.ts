import {
  nominalGenericParts,
  optionalInner,
  substituteTypeParameters,
  tupleParts,
} from "../types.ts";
import { NUMERIC_TYPES } from "../numeric.ts";
import type { ProgramCheckContext } from "./program-context.ts";

const BOUNDARY_TYPES = new Set(["bool", "char", "string", ...NUMERIC_TYPES.keys()]);

// A boundary result is `void`, a boundary value, or `Result[T, E]` whose `T`
// is either of those. `E` may be any type: a boundary `E` crosses as the
// error payload. A payload-free singleton enum needs no separate payload; any
// other `E` can be named but is refused if a host reports `.Err` at run time.
function structuralBoundaryResult(
  context: ProgramCheckContext,
  type: string,
  visiting: Set<number> = new Set(),
): boolean {
  if (BOUNDARY_TYPES.has(type)) return true;
  const optional = optionalInner(type);
  if (optional !== undefined) return structuralBoundaryResult(context, optional, visiting);
  const tuple = tupleParts(type);
  if (tuple) return tuple.every((element) => structuralBoundaryResult(context, element, visiting));
  const nominal = nominalGenericParts(type);
  if (nominal?.name === "List" && nominal.arguments.length === 1)
    return structuralBoundaryResult(context, nominal.arguments[0]!, visiting);
  const data = context.dataTypes.get(nominal?.name ?? type);
  if (!data || data.local || data.fields.some((field) => !field.public && !field.embedded))
    return false;
  if (visiting.has(data.index)) return true;
  const next = new Set(visiting).add(data.index);
  const arguments_ = nominal?.arguments ?? [];
  const substitutions = new Map(
    data.genericParameters.map((parameter, index) => [parameter, arguments_[index]!] as const),
  );
  return data.fields.every((field) =>
    structuralBoundaryResult(context, substituteTypeParameters(field.type, substitutions), next),
  );
}

function boundaryResult(context: ProgramCheckContext, type: string): boolean {
  if (type === "void" || BOUNDARY_TYPES.has(type)) return true;
  const parts = nominalGenericParts(type);
  if (parts?.name === "Result" && parts.arguments.length === 2) {
    const ok = parts.arguments[0]!;
    return ok === "void" || BOUNDARY_TYPES.has(ok);
  }
  return structuralBoundaryResult(context, type);
}

export function validateHostCapabilities(context: ProgramCheckContext): void {
  for (const capability of context.hostCapabilities) {
    const trait = context.traitTypes.get(capability);
    if (!trait) continue;
    const supported =
      trait.genericParameters.length === 0 &&
      trait.methods.every(
        (method) =>
          method.parameters.every((parameter) => BOUNDARY_TYPES.has(parameter)) &&
          boundaryResult(context, method.result) &&
          method.requirements.length === 0,
      );
    if (supported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${trait.name}' currently requires non-generic methods ` +
        "whose arguments are scalar boundary values and whose results are boundary-safe values",
      span: trait.span,
    });
  }
}
