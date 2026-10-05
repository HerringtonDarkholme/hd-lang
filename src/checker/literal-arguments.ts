import type { Expression } from "../ast.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import type { PlannedArgument } from "./context-types.ts";
import { genericTypeName, isNumericLiteralArgument, mentionsUnsolved } from "./shared.ts";
import { isLiteralStructure, literalGroupDefault } from "./literal-join.ts";
import { forcedGroupWidth } from "./literal-retry.ts";

/**
 * Checks a generic call's planned arguments in source order, except that a
 * numeric literal for a type parameter still unsolved waits, so that a later
 * argument can solve the parameter and give the literal its width, as the
 * closure does in `fold(0, fn(acc: usize, n: usize) -> usize: acc + n)`
 * (types.literal.local.form.argument).
 *
 * The waiting literals are checked at the end, or before a function argument
 * that may read the parameter: a generic function value, or a closure with an
 * unannotated parameter. A literal whose parameter nothing else solves keeps
 * the literal's own type; when every literal argument of a parameter that
 * nothing else solves is checked here, the group is `i32` if any of them is
 * signed, as in `biggest(0, -3)` (the join model, literal-join.ts).
 */
export function checkLiteralArgumentsLast(
  plan: readonly PlannedArgument[],
  sources: readonly Expression[],
  callee: {
    readonly parameters: readonly ValueType[];
    readonly genericParameters: readonly string[];
  },
  substitutions: Map<string, ValueType>,
  isGenericFunctionValue: (source: Expression) => boolean,
  check: (entry: PlannedArgument) => HirExpression,
): HirExpression[] {
  const checked: HirExpression[] = Array.from({ length: plan.length });
  const waiting: number[] = [];
  const parameterOf = (index: number): string | undefined => {
    const entry = plan[index]!;
    return genericTypeName(callee.parameters[entry.parameterIndex]!);
  };
  const checkWaiting = (): void => {
    const indices = waiting.splice(0);
    const groups = new Map<string, Expression[]>();
    for (const index of indices) {
      const parameter = parameterOf(index);
      if (parameter === undefined || substitutions.has(parameter)) continue;
      const entry = plan[index]!;
      groups.set(parameter, [
        ...(groups.get(parameter) ?? []),
        sources[entry.argumentIndices[0]!]!,
      ]);
    }
    for (const [parameter, members] of groups) {
      // One literal keeps its own type, so a bound checked with it can still decide.
      const forced = forcedGroupWidth(members);
      const type = forced ?? (members.length > 1 ? literalGroupDefault(members) : undefined);
      if (type !== undefined && type !== "u32" && type !== "f64")
        substitutions.set(parameter, type);
    }
    for (const index of indices) checked[index] = check(plan[index]!);
  };
  plan.forEach((entry, index) => {
    const source = entry.kind === "single" ? sources[entry.argumentIndices[0]!] : undefined;
    if (source === undefined) {
      checked[index] = check(entry);
      return;
    }
    const formal = callee.parameters[entry.parameterIndex]!;
    const parameter = genericTypeName(formal);
    if (
      (parameter !== undefined &&
        callee.genericParameters.includes(parameter) &&
        !substitutions.has(parameter) &&
        isNumericLiteralArgument(source)) ||
      // A list, map, or tuple of literals only waits too, as in
      // `it_each(name, [(1500, 649)], body=fn!(row: (i32, i32)): ...)`.
      (isLiteralStructure(source) &&
        mentionsUnsolved(formal, { ...callee, rowParameters: [] }, substitutions, new Map()))
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
