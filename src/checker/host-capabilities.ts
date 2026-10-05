import {
  boundaryFieldsVisible,
  boundaryShape,
  CONSENT_TRAITS,
  consentingTypes,
  isBoundaryScalar,
  isStringListArgument,
  scalarResult,
  type BoundaryDirection,
  type ConsentImplementation,
} from "../host-boundary.ts";
import { nominalGenericParts, substituteTypeParameters, displayType } from "../types.ts";
import type { ProgramCheckContext } from "./program-context.ts";

/**
 * One boundary check: the types that consent to cross in its direction, and
 * the private-field types it met without that consent
 * (spec/lang/10-modules.md#r-module.boundary.consent.error).
 */
interface BoundaryCheck {
  readonly consenting: ReadonlySet<string>;
  readonly unconsented: string[];
}

// A boundary result is `void`, a boundary value, or `Result[T, E]`. A
// `Result` whose `T` and `E` are each `void`, a scalar, or a `string`, or
// whose `E` is a payload-free singleton enum, crosses as a tag and a scalar
// payload. Any other `Result` crosses as a node tree, so both its sides must
// be boundary values; a non-generic enum is one. An argument crosses the
// same way, as a scalar, a `string`, a `List[string]`, or a node tree.
function structuralBoundaryResult(
  context: ProgramCheckContext,
  type: string,
  check: BoundaryCheck,
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
        (side) => side === "void" || structuralBoundaryResult(context, side, check, visiting),
      );
    case "enum": {
      const enumeration = shape.declaration;
      if (enumeration.local) return false;
      if (visiting.has(enumeration.name)) return true;
      const next = new Set(visiting).add(enumeration.name);
      return enumeration.variants.every((variant) =>
        variant.fields.every((field) => structuralBoundaryResult(context, field.type, check, next)),
      );
    }
    case "optional":
      return structuralBoundaryResult(context, shape.inner, check, visiting);
    case "tuple":
      return shape.elements.every((element) =>
        structuralBoundaryResult(context, element, check, visiting),
      );
    case "list":
      return structuralBoundaryResult(context, shape.element, check, visiting);
    case "data": {
      const data = shape.declaration;
      if (data.local) return false;
      if (!boundaryFieldsVisible(data, check.consenting)) {
        check.unconsented.push(data.name);
        return false;
      }
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
          check,
          next,
        ),
      );
    }
    case "other":
      return false;
  }
}

function boundaryResult(context: ProgramCheckContext, type: string, check: BoundaryCheck): boolean {
  if (type === "void" || isBoundaryScalar(type) || type === "string") return true;
  // A Result with a scalar side crosses as a tag and a scalar; any other
  // Result crosses as a node tree, so both sides must be boundary values.
  if (scalarResult(type, (name) => context.enumTypes.get(name))) return true;
  return structuralBoundaryResult(context, type, check);
}

/** A boundary argument: a scalar, a `string`, a `List[string]`, or a boundary value tree. */
function boundaryArgument(
  context: ProgramCheckContext,
  type: string,
  check: BoundaryCheck,
): boolean {
  return (
    isBoundaryScalar(type) ||
    type === "string" ||
    isStringListArgument(type) ||
    structuralBoundaryResult(context, type, check)
  );
}

/** The program's implementations, by their traits' std names. */
function implementations(context: ProgramCheckContext): ConsentImplementation[] {
  return context.program.implementations.flatMap((implementation) => {
    if (!implementation.traitName) return [];
    const head = nominalGenericParts(implementation.traitName)?.name ?? implementation.traitName;
    const standardName = context.traitTypes.get(head)?.standardName;
    return [
      {
        ...(standardName ? { traitStandardName: standardName } : {}),
        targetType: implementation.targetName,
      },
    ];
  });
}

export function validateHostCapabilities(context: ProgramCheckContext): void {
  const known = implementations(context);
  const consenting = (direction: BoundaryDirection): ReadonlySet<string> =>
    consentingTypes(known, CONSENT_TRAITS[direction]);
  const outgoing = consenting("out");
  const incoming = consenting("in");
  for (const capability of context.hostCapabilities) {
    const trait = context.traitTypes.get(capability);
    if (!trait) continue;
    let supported = trait.genericParameters.length === 0;
    let reported = false;
    for (const method of trait.methods) {
      // An argument goes out of hd, and a result comes in.
      const out: BoundaryCheck = { consenting: outgoing, unconsented: [] };
      const into: BoundaryCheck = { consenting: incoming, unconsented: [] };
      const fits =
        method.parameters.every((parameter) => boundaryArgument(context, parameter, out)) &&
        boundaryResult(context, method.result, into) &&
        method.requirements.length === 0;
      if (fits) continue;
      supported = false;
      const [direction, missing] =
        out.unconsented.length > 0
          ? (["out", out.unconsented[0]!] as const)
          : into.unconsented.length > 0
            ? (["in", into.unconsented[0]!] as const)
            : [undefined, undefined];
      if (direction === undefined) continue;
      const needed = CONSENT_TRAITS[direction].slice("std.serde.".length);
      context.diagnostics.push({
        code: "boundary-private-field",
        message:
          `'${displayType(missing)}' has a private field, so it crosses the boundary of ` +
          `'${displayType(trait.name)}.${method.name}' ${direction === "out" ? "out of" : "into"} ` +
          `hd only when it implements std.serde.${needed}`,
        span: method.span,
      });
      reported = true;
      break;
    }
    if (supported || reported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${displayType(trait.name)}' currently requires non-generic methods ` +
        "whose arguments and results are boundary-safe values",
      span: trait.span,
    });
  }
}
