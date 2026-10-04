import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import type { PlannedArgument } from "./context-types.ts";
import { genericTypeName, isNumericLiteralArgument } from "./shared.ts";

/**
 * Checks a generic call's planned arguments in source order, except that a
 * numeric literal for a type parameter still unsolved waits, so that a later
 * argument can solve the parameter and give the literal its width, as the
 * closure does in `fold(0, fn(acc: usize, n: usize) -> usize: acc + n)`
 * (types.literal.open.decide.generic).
 *
 * The waiting literals are checked at the end, or before a function argument
 * that may read the parameter: a generic function value, or a closure with an
 * unannotated parameter. A literal whose parameter nothing else solves keeps
 * the literal's own type.
 */
export function checkLiteralArgumentsLast(
  plan: readonly PlannedArgument[],
  sources: readonly Expression[],
  callee: {
    readonly parameters: readonly ValueType[];
    readonly genericParameters: readonly string[];
  },
  substitutions: ReadonlyMap<string, ValueType>,
  isGenericFunctionValue: (source: Expression) => boolean,
  check: (entry: PlannedArgument) => HirExpression,
): HirExpression[] {
  const checked: HirExpression[] = Array.from({ length: plan.length });
  const waiting: number[] = [];
  const checkWaiting = (): void => {
    for (const index of waiting.splice(0)) checked[index] = check(plan[index]!);
  };
  plan.forEach((entry, index) => {
    const source = entry.kind === "single" ? sources[entry.argumentIndices[0]!] : undefined;
    if (source === undefined) {
      checked[index] = check(entry);
      return;
    }
    const parameter = genericTypeName(callee.parameters[entry.parameterIndex]!);
    if (
      parameter !== undefined &&
      callee.genericParameters.includes(parameter) &&
      !substitutions.has(parameter) &&
      isNumericLiteralArgument(source)
    ) {
      waiting.push(index);
      return;
    }
    const mayReadWaiting =
      source.kind === "closure"
        ? source.parameters.some((written) => written.type === undefined)
        : isGenericFunctionValue(source);
    if (mayReadWaiting) checkWaiting();
    checked[index] = check(entry);
  });
  checkWaiting();
  return checked;
}
