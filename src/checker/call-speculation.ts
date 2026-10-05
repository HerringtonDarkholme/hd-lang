import type { Expression } from "../ast.ts";
import type { HirExpression } from "../hir.ts";
import { literalGroupDefault, literalNoFitMessage, pureLiteralKind } from "./literal-join.ts";

// Trial checking of call arguments against several candidate
// instantiations of one generic trait (09-traits.md#method-resolution).

export const TRIAL_STATE = Symbol("checker trial state");
export type TrialSnapshot = (value: unknown, descendants?: boolean) => void;
export interface TrialParticipant {
  [TRIAL_STATE](snapshot: TrialSnapshot, rollback: (reset: () => void) => void): void;
}

/**
 * Run a checker trial without committing any mutations. The snapshot covers
 * explicitly participating mutable state; ordinary test objects use their
 * reachable object graph. Restore in place: preexisting
 * HIR nodes and local references must retain their identities.
 *
 * Checker state consists of ordinary own properties, arrays, maps and sets;
 * adding opaque state (private slots or external mutable caches) requires
 * extending this transaction contract. Nested trials each restore their own
 * entry state. A selected candidate is checked again outside the transaction.
 */
export function speculate<T>(checker: object, check: () => T): T {
  const seen = new Map<object, boolean>();
  const restore: Array<() => void> = [];
  const snapshot: TrialSnapshot = (value, descendants = true): void => {
    if (value === null || typeof value !== "object") return;
    const previous = seen.get(value);
    if (previous === true || (previous === false && !descendants)) return;
    if (value instanceof WeakMap || value instanceof WeakSet)
      throw new Error("opaque weak collections cannot participate in checker trials");
    const participant = (value as Partial<TrialParticipant>)[TRIAL_STATE];
    seen.set(value, descendants || !!participant);
    if (!participant && value instanceof Map) {
      const entries = [...value.entries()];
      if (previous === undefined)
        restore.push(() => {
          value.clear();
          for (const [key, child] of entries) value.set(key, child);
        });
      if (descendants)
        for (const [key, child] of entries) {
          snapshot(key);
          snapshot(child);
        }
    } else if (!participant && value instanceof Set) {
      const entries = [...value];
      if (previous === undefined)
        restore.push(() => {
          value.clear();
          for (const child of entries) value.add(child);
        });
      if (descendants) for (const child of entries) snapshot(child);
    }
    const descriptors = Object.getOwnPropertyDescriptors(value);
    if (previous === undefined)
      restore.push(() => {
        for (const key of Reflect.ownKeys(value)) {
          if (!Object.hasOwn(descriptors, key)) Reflect.deleteProperty(value, key);
        }
        Object.defineProperties(value, descriptors);
      });
    if (participant) {
      participant.call(value, snapshot, (reset) => restore.push(reset));
      return;
    }
    if (descendants)
      for (const key of Reflect.ownKeys(descriptors)) {
        const descriptor = Reflect.get(descriptors, key) as PropertyDescriptor;
        if ("value" in descriptor) snapshot(descriptor.value);
      }
  };
  try {
    snapshot(checker);
    return check();
  } finally {
    for (const reset of restore.toReversed()) reset();
  }
}

/**
 * The join model's choice among the instantiations that fit a call: when
 * several fit and a numeric literal argument could take several types, the
 * literal keeps its own type (`usize`, `i32` signed, `f64`), so only those
 * instantiations stay; when none does, the message asks for the type.
 */
export function keepLiteralDefaults<T extends { readonly call?: HirExpression }>(
  fitting: readonly T[],
  expression: { readonly arguments: readonly Expression[]; readonly callee: Expression },
  receiverOffset: number,
  describe: (entry: T) => string,
): T[] | string {
  const sources = expression.arguments;
  if (fitting.length < 2 || !sources.some((source) => pureLiteralKind(source) !== undefined))
    return [...fitting];
  const kept = fitting.filter((entry) =>
    literalArgumentsUseDefaults(sources, entry.call!, receiverOffset),
  );
  const callee = expression.callee as { readonly name?: unknown };
  const name = typeof callee.name === "string" ? callee.name : "call";
  return kept.length > 0 ? kept : literalNoFitMessage(name, sources, fitting.map(describe));
}

/** True when every numeric-literal argument was checked at its default type. */
export function literalArgumentsUseDefaults(
  sources: readonly Expression[],
  call: HirExpression,
  receiverOffset = 1,
): boolean {
  const checked = "arguments" in call ? (call.arguments as readonly HirExpression[]) : [];
  return sources.every((source, index) => {
    // The literal's own default: `usize` bare, `i32` signed, `f64` for a float.
    const own = literalGroupDefault([source]);
    if (own === undefined) return true;
    return checked[index + receiverOffset]?.type === own;
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
