import {
  boundaryShape,
  isBoundaryScalar,
  isStringListArgument,
  scalarResult,
} from "../host-boundary.ts";
import { substituteTypeParameters, displayType } from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";

// A boundary result is `void`, a boundary value, or `Result[T, E]`. A
// `Result` whose `T` is `void`, a scalar, or a `string` crosses as a tag and
// a scalar payload: a boundary `E` crosses as the error payload, a
// payload-free singleton enum needs none, and any other `E` is refused if a
// host reports `.Err` at run time. Any other `Result` crosses as a node tree,
// so both its sides must be boundary values; a non-generic enum is one.
function structuralBoundaryResult(
  context: ProgramCheckContext,
  type: string,
  visiting: Set<string> = new Set(),
): boolean {
  const shape = boundaryShape(
    type,
    (name) => context.dataTypes.get(name),
    (name) => context.enumTypes.get(name),
  );
  switch (shape.kind) {
    case "scalar":
    case "string":
      return true;
    case "result":
      return [shape.ok, shape.err].every(
        (side) => side === "void" || structuralBoundaryResult(context, side, visiting),
      );
    case "enum": {
      const enumeration = shape.declaration;
      if (enumeration.local) return false;
      if (visiting.has(enumeration.name)) return true;
      const next = new Set(visiting).add(enumeration.name);
      return enumeration.variants.every((variant) =>
        variant.fields.every((field) => structuralBoundaryResult(context, field.type, next)),
      );
    }
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
  // A Result with a scalar side crosses as a tag and a scalar; any other
  // Result crosses as a node tree, so both sides must be boundary values.
  if (scalarResult(type)) return true;
  return structuralBoundaryResult(context, type);
}

/** A boundary argument: a scalar, a `string`, or a `List[string]`. */
function boundaryArgument(type: string): boolean {
  return isBoundaryScalar(type) || type === "string" || isStringListArgument(type);
}

export function validateHostCapabilities(context: ProgramCheckContext): void {
  for (const capability of context.hostCapabilities) {
    const trait = context.traitTypes.get(capability);
    if (!trait) continue;
    const supported =
      trait.genericParameters.length === 0 &&
      trait.methods.every(
        (method) =>
          method.parameters.every(boundaryArgument) &&
          boundaryResult(context, method.result) &&
          method.requirements.length === 0,
      );
    if (supported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${displayType(trait.name)}' currently requires non-generic methods ` +
        "whose arguments are scalars, strings, or List[string] and whose results are boundary-safe values",
      span: trait.span,
    });
  }
}
