import type { ValueType } from "../hir.ts";
import {
  isPermissionWeakening,
  isRowSubsumption,
  weakenBoundedGenericActual,
} from "./assignability.ts";
import { inferGenericType, mentionedGenericParameters, substituteGenericType } from "./shared.ts";
import { varianceConversion, type Declarations } from "./variance.ts";

// Typed-fact patterns (spec/lang/14-annotations.md#member-typed-facts). A
// typed fact type's pattern `Q` is matched against a type `X` exactly as the
// call `infer(annotatee)` of `fn infer[P](annotatee: Q)` matches an argument
// of type `X` (annot.typed-fact.infer, .infer.call): the argument solves the
// parameters `Q` mentions, and then converts to `Q` with them, through mut
// weakening (.infer.weaken), declared variance, or row subsumption. A
// suspending function never converts to a `fn` pattern (.infer.suspending),
// nor a function with a requirement row to an empty row (.infer.row). The
// attach-time check and a handle's `h.fact::[D]()` both use it
// (annot.handle.fact.pattern).

/**
 * The arguments that `target` gives the parameters `pattern` mentions, or
 * undefined when `target` does not fit `pattern`. `bounded` holds the
 * parameters with a bound, which a `mut` argument solves at its readonly view.
 */
export function matchFactPattern(
  pattern: ValueType,
  target: ValueType,
  bounded: ReadonlySet<string>,
  declarations: Declarations,
): Map<string, ValueType> | undefined {
  const substitutions = new Map<string, ValueType>();
  const rows = new Map<string, readonly string[]>();
  const actual = weakenBoundedGenericActual(pattern, target, bounded);
  if (inferGenericType(pattern, actual, substitutions, rows) !== undefined) return undefined;
  for (const parameter of mentionedGenericParameters(pattern))
    if (!substitutions.has(parameter)) return undefined;
  const expected = substituteGenericType(pattern, substitutions, rows);
  const converts =
    actual === expected ||
    isPermissionWeakening(actual, expected) ||
    isRowSubsumption(actual, expected) ||
    varianceConversion(actual, expected, declarations) === true;
  return converts ? substitutions : undefined;
}
