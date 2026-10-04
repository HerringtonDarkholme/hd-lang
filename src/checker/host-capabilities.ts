import { boundaryShape, isBoundaryScalar, resultSides } from "../host-boundary.ts";
import { substituteTypeParameters, displayType } from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";

// A boundary result is `void`, a boundary value, or `Result[T, E]` whose `T`
// is either of those. `E` may be any type: a boundary `E` crosses as the
// error payload. A payload-free singleton enum needs no separate payload; any
// other `E` can be named but is refused if a host reports `.Err` at run time.
function structuralBoundaryResult(
  context: ProgramCheckContext,
  type: string,
  visiting: Set<string> = new Set(),
): boolean {
  const shape = boundaryShape(type, (name) => context.dataTypes.get(name));
  switch (shape.kind) {
    case "scalar":
    case "string":
      return true;
    case "optional":
      return structuralBoundaryResult(context, shape.inner, visiting);
    case "tuple":
      return shape.elements.every((element) =>
        structuralBoundaryResult(context, element, visiting),
      );
    case "list":
      return structuralBoundaryResult(context, shape.element, visiting);
    case "data": {
      const data = shape.declaration;
      if (data.local || data.fields.some((field) => !field.public && !field.embedded)) return false;
      if (visiting.has(data.name)) return true;
      const next = new Set(visiting).add(data.name);
      const substitutions = new Map(
        data.genericParameters.map(
          (parameter, index) => [parameter, shape.arguments[index]!] as const,
        ),
      );
      return data.fields.every((field) =>
        structuralBoundaryResult(
          context,
          substituteTypeParameters(field.type, substitutions),
          next,
        ),
      );
    }
    case "other":
      return false;
  }
}

function boundaryResult(context: ProgramCheckContext, type: string): boolean {
  if (type === "void" || isBoundaryScalar(type) || type === "string") return true;
  const sides = resultSides(type);
  if (sides) {
    const ok = sides[0]!;
    return ok === "void" || isBoundaryScalar(ok) || ok === "string";
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
          method.parameters.every(
            (parameter) => isBoundaryScalar(parameter) || parameter === "string",
          ) &&
          boundaryResult(context, method.result) &&
          method.requirements.length === 0,
      );
    if (supported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${displayType(trait.name)}' currently requires non-generic methods ` +
        "whose arguments are scalar boundary values and whose results are boundary-safe values",
      span: trait.span,
    });
  }
}
