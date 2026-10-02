import type { Expression } from "../ast.ts";

/**
 * Collects the `_` placeholders that belong to a pipe step: a nested pipe's
 * step keeps its own (05-expressions.md#r-expr.pipe.slot.nested), and one
 * inside a closure is reported (05-expressions.md#r-expr.pipe.slot.closure).
 */
export function collectPipePlaceholders(
  value: unknown,
  inClosure: boolean,
  found: Expression[],
  inClosureFound: (placeholder: Expression) => never,
): void {
  if (!value || typeof value !== "object") return;
  if (Array.isArray(value)) {
    for (const item of value) collectPipePlaceholders(item, inClosure, found, inClosureFound);
    return;
  }
  const node = value as Expression;
  if (node.kind === "name" && node.name === "_") {
    if (inClosure) inClosureFound(node);
    found.push(node);
    return;
  }
  if (node.kind === "pipe") {
    collectPipePlaceholders(node.value, inClosure, found, inClosureFound);
    return;
  }
  const closure = node.kind === "closure";
  for (const [key, child] of Object.entries(node))
    if (key !== "span") collectPipePlaceholders(child, inClosure || closure, found, inClosureFound);
}

/**
 * A bare step is a name, names joined by `.`, or a method reference, with no
 * type arguments or suffix (05-expressions.md#r-expr.pipe.bare.form).
 */
export function isBarePipeStep(step: Expression): boolean {
  if (step.kind === "name") return step.typeArguments === undefined;
  if (step.kind === "member")
    return (
      step.typeArguments === undefined &&
      step.parenthesized !== true &&
      isBarePipeStep(step.receiver)
    );
  return (
    step.kind === "qualified-name" &&
    step.typeArguments === undefined &&
    step.ownerTypeArguments === undefined
  );
}
