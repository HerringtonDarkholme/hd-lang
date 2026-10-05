import { TRIAL_STATE, type TrialParticipant, type TrialSnapshot } from "./call-speculation.ts";

// These are immutable inputs to body checking, not transaction participants.
// Signature replacement is handled by LazySignatures' sparse write journal.
const INPUTS = new Set([
  "declaration",
  "signature",
  "signatures",
  "dataTypes",
  "enumTypes",
  "traitTypes",
  "implementations",
  "inherentMethods",
  "allImplementations",
  "allInherentMethods",
  "imports",
]);
const SHALLOW = new Set(["closures", "globals", "diagnostics", "rangePatternConditions"]);

/** Capture runtime state and subclass caches, never the immutable program. */
export function snapshotCheckerState(checker: object, snapshot: TrialSnapshot): undefined {
  // Read marks are trial-local: a local read only inside a rejected
  // candidate must not survive to suppress its unused-local warning
  // (CheckerContext readLocals). Snapshot the set explicitly: the generic
  // walk below would cover it, but silently, and the INPUTS list must never
  // swallow it. A later visit through the walk is a no-op (seen map).
  const reads = (checker as { readLocals?: unknown }).readLocals;
  if (reads instanceof Set) snapshot(reads);
  const descriptors = Object.getOwnPropertyDescriptors(checker);
  for (const key of Reflect.ownKeys(descriptors)) {
    const descriptor = Reflect.get(descriptors, key) as PropertyDescriptor;
    if (key === "signatures" && "value" in descriptor) {
      const signatures = descriptor.value as Partial<TrialParticipant> | undefined;
      if (signatures?.[TRIAL_STATE]) snapshot(signatures);
    }
    if ("value" in descriptor && !(typeof key === "string" && INPUTS.has(key)))
      snapshot(descriptor.value, !(typeof key === "string" && SHALLOW.has(key)));
  }
  return undefined;
}
