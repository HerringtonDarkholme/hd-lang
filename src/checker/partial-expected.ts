import type { Expression } from "../ast.ts";
import type { ValueType } from "../hir.ts";

// Partly solved expected types (07-functions.md#generic-function-values,
// #r-fn.generic.placeholder.solve). While a call checks an argument whose
// formal type still mentions the call's unsolved type parameters, those
// parameters are pending: the argument receives the formal as its expected
// type, and only the positions that mention no pending parameter count.
// A closure, a generic function value, and a call of a generic function by
// name take part. So `keep::[i64, _](sample("ids"))` solves `sample`'s `T`
// as `i64` from `keep`'s formal `Sample[i64, B]`, as a typed fact's value
// needs (14-annotations.md#r-annot.typed-fact.check.other-params).

/**
 * The value that decides whether `source` receives the partly solved
 * formal: a call by name without type arguments stands for its callee, so a
 * generic function's call receives it as that function's value would.
 */
export function calleeOf(source: Expression): Expression {
  return source.kind === "call" && !source.typeArguments && source.callee.kind === "name"
    ? source.callee
    : source;
}

/**
 * Forgets each type argument that an expected result type solved with a
 * pending parameter of the enclosing call, which only that call's other
 * arguments can solve.
 */
export function keepSolvedPositions(
  substitutions: Map<string, ValueType>,
  solved: (type: ValueType | undefined) => ValueType | undefined,
): void {
  for (const [name, type] of substitutions)
    if (solved(type) === undefined) substitutions.delete(name);
}
