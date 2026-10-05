import type { NumericFamily, ValueType } from "../hir.ts";
import { readonlyType } from "../types.ts";

/**
 * Whether the parameters of a numeric-family implementation that
 * `substitutions` solves take types their sealed bounds list, as `N < Integer`
 * holds for `i64` (09-traits.md#r-trait.target.numeric-family.each). With
 * `complete`, every parameter must be solved.
 */
export function familyHolds(
  family: NumericFamily | undefined,
  substitutions: ReadonlyMap<string, ValueType>,
  complete: boolean,
): boolean {
  if (!family) return true;
  return Object.entries(family).every(([parameter, types]) => {
    const actual = substitutions.get(parameter);
    return actual === undefined ? !complete : types.includes(readonlyType(actual));
  });
}
