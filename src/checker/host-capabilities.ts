import { nominalGenericParts } from "../types.ts";
import { NUMERIC_TYPES } from "../numeric.ts";
import type { ProgramCheckContext } from "./program-context.ts";

const BOUNDARY_TYPES = new Set(["bool", "char", "string", ...NUMERIC_TYPES.keys()]);

// A boundary result is `void`, a boundary value, or `Result[T, E]` whose `T`
// is either of those. `E` may be any type: a boundary `E` crosses as the
// error payload. A payload-free singleton enum needs no separate payload; any
// other `E` can be named but is refused if a host reports `.Err` at run time.
function boundaryResult(type: string): boolean {
  if (type === "void" || BOUNDARY_TYPES.has(type)) return true;
  const parts = nominalGenericParts(type);
  if (parts?.name !== "Result" || parts.arguments.length !== 2) return false;
  const ok = parts.arguments[0]!;
  return ok === "void" || BOUNDARY_TYPES.has(ok);
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
          boundaryResult(method.result) &&
          method.requirements.length === 0,
      );
    if (supported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${trait.name}' currently requires non-generic methods ` +
        "whose arguments and results are scalar or string boundary values",
      span: trait.span,
    });
  }
}
