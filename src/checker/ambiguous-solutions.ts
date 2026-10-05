// The parameters each substitution map's inference found several valid
// solutions for (04-type-system.md#r-types.infer.ambiguous). A parameter
// that stays unsolved is then `ambiguous-type`, not `cannot-infer-type`
// (04-type-system.md#r-types.infer.ambiguous.code). Inference copies a map
// for a trial; a mark on a discarded copy is dropped with it.

import type { ValueType } from "../hir.ts";

const ambiguousSolutions = new WeakMap<ReadonlyMap<string, ValueType>, Set<string>>();

/** Records that several solutions fit each of `parameters` in `substitutions`. */
export function markAmbiguous(
  substitutions: ReadonlyMap<string, ValueType>,
  parameters: Iterable<string>,
): void {
  const marked = ambiguousSolutions.get(substitutions) ?? new Set<string>();
  for (const parameter of parameters) marked.add(parameter);
  if (marked.size > 0) ambiguousSolutions.set(substitutions, marked);
}

/** Carries the marks of `from` to its copy `to`. */
export function carryAmbiguous(
  from: ReadonlyMap<string, ValueType>,
  to: ReadonlyMap<string, ValueType>,
): void {
  const marked = ambiguousSolutions.get(from);
  if (marked) ambiguousSolutions.set(to, new Set(marked));
}

/** Which of `unresolved` several solutions fit, as inference into `substitutions` recorded. */
export function ambiguousAmong(
  unresolved: readonly string[],
  substitutions: ReadonlyMap<string, ValueType>,
): readonly string[] {
  const marked = ambiguousSolutions.get(substitutions);
  return marked ? unresolved.filter((parameter) => marked.has(parameter)) : [];
}
