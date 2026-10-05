import type { Expression } from "../ast.ts";
import { sourceSpanKey } from "../diagnostics.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  DBG_INTRINSIC,
  DBG_TEXT_INTRINSIC,
  debugPrinter,
  debugPrintingCall,
  recordDebugSite,
  registeredDebugPrint,
  type DebugPrinter,
} from "./debug-print.ts";
import { InspectChecker } from "./expression-inspect.ts";
import { FACTS_OF_INTRINSIC } from "./function-facts.ts";

interface NamedCallExpression extends Extract<Expression, { kind: "call" }> {
  readonly callee: Extract<Expression, { kind: "name" }>;
}

/** Calls of the std functions that the checker lowers itself: `facts_of` and `dbg`. */
export abstract class DebugPrintChecker extends InspectChecker {
  protected abstract checkDeclaredCall(
    expression: NamedCallExpression,
    expected?: ValueType,
  ): HirExpression;

  /** A call of `facts_of`, `dbg`, or the REPL's `dbg_text`; undefined for any other callee. */
  protected checkIntrinsicFunctionCall(
    expression: NamedCallExpression,
    expected?: ValueType,
  ): HirExpression | undefined {
    const intrinsic = this.visibleSignature(expression.callee.name)?.intrinsic;
    if (intrinsic === FACTS_OF_INTRINSIC)
      return this.checkExpression(this.factsOfCall(expression), expected);
    if (intrinsic === DBG_INTRINSIC || intrinsic === DBG_TEXT_INTRINSIC)
      return this.checkDebugPrintCall(expression, intrinsic === DBG_TEXT_INTRINSIC);
    return undefined;
  }

  /**
   * Whether `type` implements the standard `Debug` here, with its
   * implementation's bounds met and a type parameter's bounds in scope
   * (spec/lang/10-modules.md#r-module.dbg.value.debug).
   */
  private implementsDebug(type: ValueType): boolean {
    for (const trait of this.traitTypes.values())
      if (trait.standardName === "std.format.Debug")
        return this.traitMethodDispatch(type, trait.name) !== undefined;
    return false;
  }

  /**
   * A `dbg` call is an ordinary call of its declaration
   * (spec/lang/10-modules.md#r-module.dbg.signature), whose body prints
   * nothing. In the user's own code the second pass swaps in the printing
   * body (`debugPrintingCall`), which needs each argument's static type, so
   * the first pass records the types that the ordinary check collected. The
   * REPL's `dbg_text` call takes the same two passes.
   */
  private checkDebugPrintCall(expression: NamedCallExpression, text: boolean): HirExpression {
    if (
      expression.typeArguments?.length ||
      expression.callee.typeArguments?.length ||
      expression.argumentSpreads?.some(Boolean) ||
      expression.argumentNames?.some((name) => name !== undefined)
    )
      this.fail(
        "invalid-dbg-call",
        "dbg takes positional arguments only, with no type arguments and no spread",
        expression.span,
      );
    const state = registeredDebugPrint(this.traitTypes);
    const key = sourceSpanKey(expression.span);
    const fetched = text || !state ? undefined : state.fetchedPackage(expression.span);
    if (fetched !== undefined && state) {
      // A fetched dependency's call prints nothing, and its package warns
      // once (spec/lang/10-modules.md#r-module.dbg.dependency).
      if (!state.quiet.has(fetched)) state.quiet.set(fetched, expression.span);
    }
    if (!state || fetched !== undefined) return this.checkDeclaredCall(expression);
    if (!text && state.release && !state.plans) {
      // spec/lang/10-modules.md#r-module.dbg.release, with its fix-it
      // (r-module.dbg.release.delete).
      this.diagnostics.push({
        code: "dbg-in-release",
        message: "a release build cannot hold a dbg call; delete the statement",
        span: expression.span,
        fix: {
          message: "delete the dbg statement",
          edits: [{ span: expression.span, replacement: "" }],
        },
      });
    }
    const plan = state.plans?.get(key);
    if (plan) return this.checkExpression(debugPrintingCall(state, expression, plan, text));
    const checked = this.checkDeclaredCall(expression);
    const types = text
      ? checked.kind === "call"
        ? checked.arguments.slice(0, 1).map((argument) => argument.type)
        : []
      : this.collectedTypes(checked);
    const values = text ? 1 : expression.arguments.length;
    if (types.length !== values) return checked;
    const oracle = {
      implementsDebug: (type: ValueType) => this.implementsDebug(type),
      dataTypes: this.dataTypes,
      enumTypes: this.enumTypes,
    };
    const printers: DebugPrinter[] = types.map((type, index) =>
      debugPrinter(type, oracle, state.hasDebug, this.namedFunction(expression.arguments[index]!)),
    );
    recordDebugSite(state, expression.span, key, text, printers);
    return checked;
  }

  /** The static type of each value that an ordinary `dbg` call collected into `Args`. */
  private collectedTypes(checked: HirExpression): readonly ValueType[] {
    const collected = checked.kind === "call" ? checked.arguments[0] : undefined;
    return collected?.kind === "tuple" ? collected.elementTypes : [];
  }

  /** The function that `value` names, when it names one rather than a local value. */
  private namedFunction(value: Expression): string | undefined {
    if (value.kind !== "name") return undefined;
    if (
      this.resolveLocal(value.name) ||
      this.availableCaptures.has(value.name) ||
      this.resolveGlobal(value.name)
    )
      return undefined;
    return this.visibleSignature(value.name) ? value.name : undefined;
  }
}
