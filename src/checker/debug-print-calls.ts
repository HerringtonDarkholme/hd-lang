import type { Expression } from "../ast.ts";
import { sourceSpanKey } from "../diagnostics.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import {
  DBG_INTRINSIC,
  DBG_TEXT_INTRINSIC,
  debugPrinter,
  debugPrintingCall,
  debugQuietCall,
  debugReleaseReplacement,
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
  /** A call of `facts_of`, `dbg`, or the REPL's `dbg_text`; undefined for any other callee. */
  protected checkIntrinsicFunctionCall(
    expression: NamedCallExpression,
    expected?: ValueType,
  ): HirExpression | undefined {
    const intrinsic = this.visibleSignature(expression.callee.name)?.intrinsic;
    if (intrinsic === FACTS_OF_INTRINSIC)
      return this.checkExpression(this.factsOfCall(expression), expected);
    if (intrinsic === DBG_INTRINSIC || intrinsic === DBG_TEXT_INTRINSIC)
      return this.checkDebugPrintCall(expression, intrinsic === DBG_TEXT_INTRINSIC, expected);
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
   * A `dbg` call, or the REPL's `dbg_text` call when `text`
   * (spec/lang/10-modules.md#debug-printing, checker/debug-print.ts). The
   * first pass checks its arguments alone and records their types; the
   * second checks it as calls of `std.format`'s printing functions. A call in
   * a fetched dependency stays its arguments alone in both.
   */
  private checkDebugPrintCall(
    expression: NamedCallExpression,
    text: boolean,
    context?: ValueType,
  ): HirExpression {
    // A `dbg(x)` statement returns `x` to no one, so `void` constrains nothing
    // (spec/lang/10-modules.md#r-module.dbg.one.expected).
    const expected = context === "void" ? undefined : context;
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
    if (!state) return this.checkExpression(debugQuietCall(expression), expected);
    const key = sourceSpanKey(expression.span);
    const fetched = text ? undefined : state.fetchedPackage(expression.span);
    if (fetched !== undefined) {
      // A fetched dependency's call prints nothing, and its package warns
      // once (spec/lang/10-modules.md#r-module.dbg.dependency).
      if (!state.quiet.has(fetched)) state.quiet.set(fetched, expression.span);
      return this.checkExpression(debugQuietCall(expression), expected);
    }
    if (!text && state.release && !state.plans) {
      // spec/lang/10-modules.md#r-module.dbg.release, with its fix-it.
      const replacement = debugReleaseReplacement(state, expression);
      this.diagnostics.push({
        code: "dbg-in-release",
        message:
          replacement === ""
            ? "a release build cannot hold a dbg call; delete it"
            : `a release build cannot hold a dbg call; write '${replacement}' instead`,
        span: expression.span,
        fix: {
          message:
            replacement === "" ? "delete the dbg call" : `replace the call with '${replacement}'`,
          edits: [{ span: expression.span, replacement }],
        },
      });
    }
    const plan = state.plans?.get(key);
    if (plan)
      return this.checkExpression(debugPrintingCall(state, expression, plan, text), expected);
    const values = text ? expression.arguments.slice(0, 1) : expression.arguments;
    const checked = text
      ? this.checkExpression(values[0]!)
      : this.checkExpression(debugQuietCall(expression), expected);
    const types =
      values.length === 0
        ? []
        : values.length === 1
          ? [checked.type]
          : checked.kind === "tuple"
            ? checked.elementTypes
            : [];
    if (types.length !== values.length) return checked;
    const oracle = {
      implementsDebug: (type: ValueType) => this.implementsDebug(type),
      dataTypes: this.dataTypes,
      enumTypes: this.enumTypes,
    };
    const printers: DebugPrinter[] = values.map((value, index) =>
      debugPrinter(types[index]!, oracle, state.hasDebug, this.namedFunction(value)),
    );
    recordDebugSite(state, expression.span, key, text, printers);
    if (!text) return checked;
    return this.checkExpression({ kind: "string", value: "", span: expression.span });
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
