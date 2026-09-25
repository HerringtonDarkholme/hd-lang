import type { ProgramCheckContext } from "./program-context.ts";

export function validateHostCapabilities(context: ProgramCheckContext): void {
  const boundaryTypes = new Set(["bool", "char", "f64", "i32", "string"]);
  for (const capability of context.hostCapabilities) {
    const trait = context.traitTypes.get(capability);
    if (!trait) continue;
    const supported =
      trait.genericParameters.length === 0 &&
      trait.methods.every(
        (method) =>
          method.suspending &&
          method.parameters.every((parameter) => boundaryTypes.has(parameter)) &&
          (method.result === "void" || boundaryTypes.has(method.result)) &&
          method.requirements.length === 0,
      );
    if (supported) continue;
    context.diagnostics.push({
      code: "unsupported-host-provider-signature",
      message:
        `host capability '${trait.name}' currently requires non-generic suspending methods ` +
        "whose arguments and results are scalar or string boundary values",
      span: trait.span,
    });
  }
}
