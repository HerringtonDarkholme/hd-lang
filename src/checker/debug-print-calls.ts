import type { Expression } from "../ast.ts";
import { sourceSpanKey, type SourceSpan } from "../diagnostics.ts";
import type { HirExpression, ValueType } from "../hir.ts";
import { functionParts, readonlyType, restInner, tupleParts } from "../types.ts";
import {
  DBG_INTRINSIC,
  DBG_TEXT_INTRINSIC,
  debugPrinter,
  debugPrintingCall,
  recordDebugSite,
  registeredDebugPrint,
  type DebugPrinter,
  type DebugSiteMode,
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
    const state = registeredDebugPrint(this.traitTypes);
    const key = sourceSpanKey(expression.span);
    const fetched = text || !state ? undefined : state.fetchedPackage(expression.span);
    if (fetched !== undefined && state) {
      // A fetched dependency's call prints nothing, and its package warns
      // once (spec/lang/10-modules.md#r-module.dbg.dependency).
      if (!state.quiet.has(fetched)) state.quiet.set(fetched, expression.span);
    }
    if (!state || fetched !== undefined) return this.checkDeclaredCall(expression);
    // A function value of `dbg` is adapted by a call at the value's own span;
    // that call is the value's body, not a call site (checkDebugFunctionValue).
    const site = state.plans?.get(key) ?? state.sites.get(key);
    if (site?.mode === "value") return this.checkDeclaredCall(expression);
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
    if (plan) return this.checkExpression(debugPrintingCall(state, expression, plan));
    const checked = this.checkDeclaredCall(expression);
    // Each argument has its own source text only in a call of positional
    // arguments with no spread, no `values=`, and no type arguments.
    const bare =
      !text &&
      (expression.typeArguments?.length ||
        expression.callee.typeArguments?.length ||
        expression.argumentSpreads?.some(Boolean) ||
        expression.argumentNames?.some((name) => name !== undefined));
    const types = text
      ? checked.kind === "call"
        ? checked.arguments.slice(0, 1).map((argument) => argument.type)
        : []
      : this.collectedTypes(checked);
    if (!bare && types.length !== (text ? 1 : expression.arguments.length)) return checked;
    this.recordDebugSite(
      state,
      expression,
      key,
      text ? "text" : bare ? "bare" : "positional",
      types,
      !bare,
    );
    return checked;
  }

  /**
   * `dbg` used as a function value has no call site. Its type comes from the
   * expected function type, and the second pass names a generated function
   * that prints each argument alone (spec/lang/10-modules.md#r-module.dbg.body.site).
   * Returns undefined to let the plain rules check the value.
   */
  protected checkDebugFunctionValue(
    expression: Extract<Expression, { kind: "name" }>,
    expected: ValueType | undefined,
  ): HirExpression | undefined {
    const state = registeredDebugPrint(this.traitTypes);
    if (!state || state.fetchedPackage(expression.span) !== undefined) return undefined;
    if (state.release && !state.plans)
      // spec/lang/10-modules.md#r-module.dbg.release: any reference to `dbg`
      // is rejected. The value's user must change, so there is no fix-it.
      this.diagnostics.push({
        code: "dbg-in-release",
        message: "a release build cannot refer to dbg; change the code that uses the value",
        span: expression.span,
      });
    const plan = state.plans?.get(sourceSpanKey(expression.span));
    if (plan?.site)
      return this.checkExpression({ kind: "name", name: plan.site, span: expression.span });
    // A variadic function's value takes its collected arguments as one
    // tuple (spec/lang/07-functions.md#function-values), so the expected
    // type is `fn((A, B)) -> void`, and the site prints each element.
    const parts = expected === undefined ? undefined : functionParts(expected);
    const collected = parts?.parameters.length === 1 ? tupleParts(parts.parameters[0]!) : undefined;
    if (collected && !state.plans && !collected.some((part) => restInner(part) !== undefined))
      this.recordDebugSite(
        state,
        expression,
        sourceSpanKey(expression.span),
        "value",
        collected,
        false,
      );
    return undefined;
  }

  private recordDebugSite(
    state: NonNullable<ReturnType<typeof registeredDebugPrint>>,
    expression: { readonly span: SourceSpan; readonly arguments?: readonly Expression[] },
    key: string,
    mode: DebugSiteMode,
    types: readonly ValueType[],
    named: boolean,
  ): void {
    const oracle = {
      implementsDebug: (type: ValueType) => this.implementsDebug(type),
      dataTypes: this.dataTypes,
      enumTypes: this.enumTypes,
    };
    const printers: DebugPrinter[] = types.map((type, index) =>
      debugPrinter(
        type,
        oracle,
        state.hasDebug,
        named ? this.namedFunction(expression.arguments![index]!) : undefined,
      ),
    );
    recordDebugSite(state, expression.span, key, mode, printers, types);
  }

  /** The static type of each value that an ordinary `dbg` call collected into `Args`. */
  private collectedTypes(checked: HirExpression): readonly ValueType[] {
    const collected = checked.kind === "call" ? checked.arguments[0] : undefined;
    const parts = collected ? tupleParts(readonlyType(collected.type)) : undefined;
    return parts && !parts.some((part) => restInner(part) !== undefined) ? parts : [];
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
