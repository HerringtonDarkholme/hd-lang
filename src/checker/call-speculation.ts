import type { Expression } from "../ast.ts";
import type { HirExpression } from "../hir.ts";

// Trial checking of call arguments against several candidate
// instantiations of one generic trait (09-traits.md#method-resolution).

/**
 * Run a checker trial without committing any mutations. The snapshot covers
 * the checker's entire reachable object graph, including shared closures,
 * captures, locals, globals, and subclass caches. Restore in place: preexisting
 * HIR nodes and local references must retain their identities.
 *
 * Checker state consists of ordinary own properties, arrays, maps and sets;
 * adding opaque state (private slots or external mutable caches) requires
 * extending this transaction contract. Nested trials each restore their own
 * entry state. A selected candidate is checked again outside the transaction.
 */
export function speculate<T>(checker: object, check: () => T): T {
  const seen = new Set<object>();
  const restore: Array<() => void> = [];
  const snapshot = (value: unknown): void => {
    if (value === null || typeof value !== "object" || seen.has(value)) return;
    seen.add(value);
    if (value instanceof WeakMap || value instanceof WeakSet)
      throw new Error("opaque weak collections cannot participate in checker trials");
    if (value instanceof Map) {
      const entries = [...value.entries()];
      restore.push(() => {
        value.clear();
        for (const [key, child] of entries) value.set(key, child);
      });
      for (const [key, child] of entries) {
        snapshot(key);
        snapshot(child);
      }
    } else if (value instanceof Set) {
      const entries = [...value];
      restore.push(() => {
        value.clear();
        for (const child of entries) value.add(child);
      });
      for (const child of entries) snapshot(child);
    }
    const descriptors = Object.getOwnPropertyDescriptors(value);
    restore.push(() => {
      for (const key of Reflect.ownKeys(value)) {
        if (!Object.hasOwn(descriptors, key)) Reflect.deleteProperty(value, key);
      }
      Object.defineProperties(value, descriptors);
    });
    for (const key of Reflect.ownKeys(descriptors)) {
      const descriptor = Reflect.get(descriptors, key) as PropertyDescriptor;
      if ("value" in descriptor) snapshot(descriptor.value);
    }
  };
  snapshot(checker);
  try {
    return check();
  } finally {
    for (const reset of restore.toReversed()) reset();
  }
}

/** True when every numeric-literal argument was checked at its default type. */
export function literalArgumentsUseDefaults(
  sources: readonly Expression[],
  call: HirExpression,
  receiverOffset = 1,
): boolean {
  const checked = "arguments" in call ? (call.arguments as readonly HirExpression[]) : [];
  return sources.every((source, index) => {
    const literal = source.kind === "unary" && source.operator === "-" ? source.operand : source;
    if (literal.kind !== "integer" && literal.kind !== "float") return true;
    const type = checked[index + receiverOffset]?.type;
    return type === (literal.kind === "integer" ? "i32" : "f64");
  });
}

// Legacy collection-row rechecking is not a candidate transaction. Keep its
// existing guard until collection inference is repaired separately; method
// selection never uses this syntax filter.
const COLLECTION_RECHECK_UNSAFE_KINDS = new Set([
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

export function speculationSafeArguments(value: unknown): boolean {
  if (Array.isArray(value)) return value.every(speculationSafeArguments);
  if (value === null || typeof value !== "object") return true;
  const kind = (value as { kind?: unknown }).kind;
  if (typeof kind === "string" && COLLECTION_RECHECK_UNSAFE_KINDS.has(kind)) return false;
  return Object.entries(value).every(
    ([key, child]) => key === "span" || speculationSafeArguments(child),
  );
}
