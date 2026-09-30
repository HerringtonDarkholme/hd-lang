import type { Expression } from "../ast.ts";
import type { HirExpression } from "../hir.ts";

// Trial checking of call arguments against several candidate
// instantiations of one generic trait (09-traits.md#method-resolution).

const SPECULATION_UNSAFE_KINDS = new Set([
  "binding-expression",
  "closure",
  "list-comprehension",
  "map-comprehension",
  "if",
  "for",
  "while",
  "match",
  "pipe",
  "provider-context",
  "provider-with",
  "suspend-call",
]);

/**
 * Whether call arguments can be checked once per candidate without lasting
 * effects: no nested scopes, bindings, or closures.
 */
/** True when every numeric-literal argument was checked at its default type. */
export function literalArgumentsUseDefaults(
  sources: readonly Expression[],
  call: HirExpression,
): boolean {
  const checked = "arguments" in call ? (call.arguments as readonly HirExpression[]) : [];
  return sources.every((source, index) => {
    const literal = source.kind === "unary" && source.operator === "-" ? source.operand : source;
    if (literal.kind !== "integer" && literal.kind !== "float") return true;
    const type = checked[index + 1]?.type;
    return type === (literal.kind === "integer" ? "i32" : "f64");
  });
}

export function speculationSafeArguments(value: unknown): boolean {
  if (Array.isArray(value)) return value.every(speculationSafeArguments);
  if (value === null || typeof value !== "object") return true;
  const kind = (value as { kind?: unknown }).kind;
  if (typeof kind === "string" && SPECULATION_UNSAFE_KINDS.has(kind)) return false;
  return Object.entries(value).every(
    ([key, child]) => key === "span" || speculationSafeArguments(child),
  );
}
