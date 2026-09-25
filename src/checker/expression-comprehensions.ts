import type { BindingName, ComprehensionClause, Expression } from "../ast.ts";
import type { HirComprehensionClause, HirExpression, HirLocal, ValueType } from "../hir.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
  tupleParts,
} from "../types.ts";
import { mapKeyKind, PRELUDE_NAMES } from "./context.ts";
import { ExpressionDataChecker } from "./expression-data.ts";
import { findSuspensionCall } from "./program-effects.ts";
import { iterableInfo } from "./shared.ts";

interface CheckedIterableInfo {
  readonly iterable: HirExpression;
  readonly iteratorFunctionIndex?: number;
  readonly iteratorKind: "iterator" | "list" | "map" | "trait";
  readonly yieldType: ValueType;
}

export abstract class ExpressionComprehensionChecker extends ExpressionDataChecker {
  protected checkComprehensionExpression(
    expression: Expression,
    expected?: ValueType,
  ): HirExpression | undefined {
    if (expression.kind !== "list-comprehension" && expression.kind !== "map-comprehension")
      return undefined;
    const suspensionCall = findSuspensionCall(expression);
    if (suspensionCall)
      this.fail(
        "suspension-forbidden-context",
        "a comprehension cannot contain a suspension call",
        suspensionCall.span,
      );
    this.scopes.push(new Map());
    try {
      const clauses = expression.clauses.map((clause) => this.checkClause(clause));
      return expression.kind === "list-comprehension"
        ? this.checkListComprehension(expression, clauses, expected)
        : this.checkMapComprehension(expression, clauses, expected);
    } finally {
      this.scopes.pop();
    }
  }

  private checkListComprehension(
    expression: Extract<Expression, { kind: "list-comprehension" }>,
    clauses: readonly HirComprehensionClause[],
    expected?: ValueType,
  ): HirExpression {
    const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
    const contextualElement =
      expectedNominal?.name === "list" && expectedNominal.arguments.length === 1
        ? expectedNominal.arguments[0]
        : undefined;
    const checkedValue = this.checkExpression(expression.value, contextualElement);
    const elementType = contextualElement ?? checkedValue.type;
    const value = this.requireCoercion(checkedValue, elementType, expression.value.span);
    const readonlyList = nominalGenericType("list", [elementType]);
    const type =
      (expected && mutableInner(expected) !== undefined) || expected === undefined
        ? mutableType(readonlyList)
        : readonlyList;
    return {
      kind: "list-comprehension",
      clauses,
      value,
      elementType,
      type,
      span: expression.span,
    };
  }

  private checkMapComprehension(
    expression: Extract<Expression, { kind: "map-comprehension" }>,
    clauses: readonly HirComprehensionClause[],
    expected?: ValueType,
  ): HirExpression {
    const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
    const contextualKey =
      expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
        ? expectedNominal.arguments[0]
        : undefined;
    const contextualValue =
      expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
        ? expectedNominal.arguments[1]
        : undefined;
    const checkedKey = this.checkExpression(expression.key, contextualKey);
    const keyType = contextualKey ?? checkedKey.type;
    const key = this.requireCoercion(checkedKey, keyType, expression.key.span);
    const checkedValue = this.checkExpression(expression.value, contextualValue);
    const valueType = contextualValue ?? checkedValue.type;
    const value = this.requireCoercion(checkedValue, valueType, expression.value.span);
    const keyKind = mapKeyKind(keyType);
    if (keyKind === undefined)
      this.fail(
        "unsupported-map-key",
        `type '${keyType}' does not have the MVP's built-in Eq and Hash support`,
        expression.key.span,
      );
    const readonlyMap = nominalGenericType("map", [keyType, valueType]);
    const type =
      (expected && mutableInner(expected) !== undefined) || expected === undefined
        ? mutableType(readonlyMap)
        : readonlyMap;
    return {
      kind: "map-comprehension",
      clauses,
      key,
      value,
      keyType,
      valueType,
      keyKind,
      type,
      span: expression.span,
    };
  }

  private checkClause(clause: ComprehensionClause): HirComprehensionClause {
    if (clause.kind === "if") {
      const condition = this.checkExpression(clause.condition);
      this.requireType(condition.type, "bool", condition.span);
      return { kind: "if", condition, span: clause.span };
    }
    const info = this.checkIterable(clause.iterable);
    const bindingTypes =
      clause.bindings.length === 1 ? [info.yieldType] : tupleParts(info.yieldType);
    if (!bindingTypes || bindingTypes.length !== clause.bindings.length)
      this.fail(
        "for-binding-arity",
        `comprehension binding has ${clause.bindings.length} names but '${info.yieldType}' yields ${bindingTypes?.length ?? 1} value${bindingTypes?.length === 1 ? "" : "s"}`,
        clause.span,
      );
    const bindings = clause.bindings.map((binding, index) =>
      this.addComprehensionBinding(binding, bindingTypes[index]!),
    );
    return { kind: "for", ...info, bindings, span: clause.span };
  }

  private checkIterable(expression: Expression): CheckedIterableInfo {
    const iterable = this.checkExpression(expression);
    const info = iterableInfo(iterable, this.implementations);
    if (info?.iteratorKind === "trait" && mutableInner(iterable.type) === undefined)
      this.fail(
        "mutable-receiver-required",
        "iteration requires mutable access to an Iterator implementation",
        expression.span,
      );
    if (info) return { iterable, ...info };
    this.fail(
      "not-iterable",
      `type '${iterable.type}' does not implement the MVP iteration protocol`,
      expression.span,
    );
  }

  private addComprehensionBinding(binding: BindingName, type: ValueType): HirLocal {
    if (this.currentScope().has(binding.name))
      this.fail(
        "duplicate-binding",
        `comprehension binding '${binding.name}' appears more than once`,
        binding.span,
      );
    if (PRELUDE_NAMES.has(binding.name))
      this.fail(
        "prelude-name-shadow",
        `comprehension binding '${binding.name}' shadows a prelude name`,
        binding.span,
      );
    const local: HirLocal = {
      name: binding.name,
      type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: binding.span,
    };
    this.locals.push(local);
    this.currentScope().set(binding.name, local);
    return local;
  }
}
